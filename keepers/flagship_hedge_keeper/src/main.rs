//! Flagship worst-of-3 hedge keeper — **scaffold pass** (audit F1).
//!
//! Reads `AggregateDelta` and `Regression`, enforces the dual-staleness
//! surface documented in `integration_architecture.md §2.6`, composes the
//! 2D SPY/QQQ hedge target using the IWM projection, decides whether the
//! bands have been breached, and in `--dry-run` mode logs the target
//! without submitting a transaction.
//!
//! Live Jupiter routing + `prepare_hedge_swap` → Jupiter swap
//! → `record_hedge_trade` composition is deferred to a follow-up pass.
//! The SOL Autocall hedge keeper (`keepers/hedge_keeper/`) already
//! implements that pattern; the flagship keeper will adopt it once the
//! scaffolding here is exercised against devnet.
//!
//! **Flagship pause state.** Until the live-submit path lands *and* has
//! completed a full devnet rebalance cycle end-to-end, flagship stays
//! paused-public. `docs/audit/OPEN_QUESTIONS.md` records the unpause
//! predicate.

use anchor_lang::AccountDeserialize;
use anyhow::{Context, Result};
use clap::Parser;
use halcyon_client_sdk::{
    decode::{fetch_anchor_account, fetch_anchor_account_opt},
    pda,
};
use pyth_solana_receiver_sdk::price_update::{PriceUpdateV2, VerificationLevel};
use serde::Deserialize;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig, pubkey::Pubkey, signature::Keypair, signer::Signer,
};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

const SCALE_6: i64 = 1_000_000;
const SCALE_12: i128 = 1_000_000_000_000;

// --- Staleness / drift thresholds (integration_architecture.md §2.6) ---
const DEFAULT_AGGREGATE_DELTA_MAX_AGE_SECS: i64 = 30 * 60; // 30 min
const DEFAULT_AGGREGATE_DELTA_MAX_SPOT_DRIFT_BPS: u64 = 50; // 0.5%
const DEFAULT_REGRESSION_MAX_AGE_SECS: i64 = 5 * 86_400; // 5 days
const DEFAULT_REBALANCE_COOLDOWN_SECS: i64 = 5 * 86_400; // cadence floor
const DEFAULT_REBALANCE_BREACH_MULTIPLE_BPS: u64 = 15_000; // 1.5× band_width

// --- Band widths (per-leg, bps of notional) — starting defaults ---
/// Each leg rebalances when |current − target| exceeds this fraction of
/// the absolute target position. Matches the 10% default used in the SOL
/// Autocall hedge controller; tunable via config.
const DEFAULT_LEG_BAND_WIDTH_BPS: u64 = 1_000;
/// Minimum trade size below which a rebalance is skipped to avoid
/// excessive fees. Fraction of absolute target position.
const DEFAULT_LEG_MIN_TRADE_BPS: u64 = 100; // 1%

#[derive(Parser, Debug)]
#[command(
    name = "flagship_hedge_keeper",
    about = "Halcyon flagship worst-of-3 hedge keeper (scaffold)"
)]
struct Args {
    #[arg(long, default_value = "config/flagship_hedge_keeper.json")]
    config: String,

    /// Run one cycle and exit. Useful for cron and smoke tests.
    #[arg(long)]
    once: bool,

    /// Dry-run mode: compute the target and log it, but do not submit
    /// any transaction. The scaffold pass **always** behaves as if this
    /// flag is set; live-submit mode lands in a follow-up. Keeping the
    /// flag here so operator tooling stays stable across the upgrade.
    #[arg(long, default_value_t = true)]
    dry_run: bool,
}

#[derive(Debug, Deserialize)]
struct KeeperConfig {
    rpc_endpoint: String,
    keypair_path: String,
    pyth_spy: String,
    pyth_qqq: String,
    pyth_iwm: String,
    #[serde(default = "default_scan_interval_secs")]
    scan_interval_secs: u64,
    #[serde(default = "default_backoff_cap_secs")]
    backoff_cap_secs: u64,
    #[serde(default = "default_failure_budget")]
    failure_budget: u32,
    #[serde(default = "default_aggregate_delta_max_age_secs")]
    aggregate_delta_max_age_secs: i64,
    #[serde(default = "default_aggregate_delta_max_spot_drift_bps")]
    aggregate_delta_max_spot_drift_bps: u64,
    #[serde(default = "default_regression_max_age_secs")]
    regression_max_age_secs: i64,
    #[serde(default = "default_rebalance_cooldown_secs")]
    rebalance_cooldown_secs: i64,
    #[serde(default = "default_rebalance_breach_multiple_bps")]
    rebalance_breach_multiple_bps: u64,
    #[serde(default = "default_leg_band_width_bps")]
    leg_band_width_bps: u64,
    #[serde(default = "default_leg_min_trade_bps")]
    leg_min_trade_bps: u64,
}

fn default_scan_interval_secs() -> u64 {
    300
}
fn default_backoff_cap_secs() -> u64 {
    60
}
fn default_failure_budget() -> u32 {
    5
}
fn default_aggregate_delta_max_age_secs() -> i64 {
    DEFAULT_AGGREGATE_DELTA_MAX_AGE_SECS
}
fn default_aggregate_delta_max_spot_drift_bps() -> u64 {
    DEFAULT_AGGREGATE_DELTA_MAX_SPOT_DRIFT_BPS
}
fn default_regression_max_age_secs() -> i64 {
    DEFAULT_REGRESSION_MAX_AGE_SECS
}
fn default_rebalance_cooldown_secs() -> i64 {
    DEFAULT_REBALANCE_COOLDOWN_SECS
}
fn default_rebalance_breach_multiple_bps() -> u64 {
    DEFAULT_REBALANCE_BREACH_MULTIPLE_BPS
}
fn default_leg_band_width_bps() -> u64 {
    DEFAULT_LEG_BAND_WIDTH_BPS
}
fn default_leg_min_trade_bps() -> u64 {
    DEFAULT_LEG_MIN_TRADE_BPS
}

impl KeeperConfig {
    fn load(path: &str) -> Result<Self> {
        let raw = std::fs::read_to_string(Path::new(path))
            .with_context(|| format!("reading flagship-hedge-keeper config at {path}"))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("parsing flagship-hedge-keeper config at {path}"))
    }

    fn load_keypair(&self) -> Result<Keypair> {
        solana_sdk::signer::keypair::read_keypair_file(&self.keypair_path)
            .map_err(|e| anyhow::anyhow!("reading keypair at {}: {}", self.keypair_path, e))
    }
}

struct KeeperClient {
    rpc: Arc<RpcClient>,
    signer: Keypair,
    pyth_spy: Pubkey,
    pyth_qqq: Pubkey,
    pyth_iwm: Pubkey,
}

impl KeeperClient {
    async fn connect(cfg: &KeeperConfig) -> Result<Self> {
        let rpc = Arc::new(RpcClient::new_with_commitment(
            cfg.rpc_endpoint.clone(),
            CommitmentConfig::confirmed(),
        ));
        rpc.get_slot()
            .await
            .with_context(|| format!("pinging RPC at {}", cfg.rpc_endpoint))?;
        Ok(Self {
            rpc,
            signer: cfg.load_keypair()?,
            pyth_spy: Pubkey::from_str(&cfg.pyth_spy)
                .with_context(|| format!("parsing pyth_spy {}", cfg.pyth_spy))?,
            pyth_qqq: Pubkey::from_str(&cfg.pyth_qqq)
                .with_context(|| format!("parsing pyth_qqq {}", cfg.pyth_qqq))?,
            pyth_iwm: Pubkey::from_str(&cfg.pyth_iwm)
                .with_context(|| format!("parsing pyth_iwm {}", cfg.pyth_iwm))?,
        })
    }
}

/// 2D hedge target in SCALE_6 units.
///
/// ```text
///   target_SPY = Δ_SPY + β_SPY · Δ_IWM
///   target_QQQ = Δ_QQQ + β_QQQ · Δ_IWM
/// ```
///
/// All arithmetic is fixed-point i128 at SCALE_12 for the β multiplication
/// so nothing leaks into f64 in the keeper's decision path. `Regression`
/// holds `beta_*_s12` already at i128 SCALE_12.
#[derive(Debug, Clone, Copy)]
pub struct HedgeTarget {
    pub target_spy_s6: i64,
    pub target_qqq_s6: i64,
}

pub fn compose_hedge_target_s6(
    delta_spy_s6: i64,
    delta_qqq_s6: i64,
    delta_iwm_s6: i64,
    beta_spy_s12: i128,
    beta_qqq_s12: i128,
) -> Result<HedgeTarget> {
    Ok(HedgeTarget {
        target_spy_s6: compose_leg_s6(delta_spy_s6, delta_iwm_s6, beta_spy_s12)?,
        target_qqq_s6: compose_leg_s6(delta_qqq_s6, delta_iwm_s6, beta_qqq_s12)?,
    })
}

fn compose_leg_s6(leg_delta_s6: i64, iwm_delta_s6: i64, beta_s12: i128) -> Result<i64> {
    let iwm = i128::from(iwm_delta_s6);
    let scaled = iwm.checked_mul(beta_s12).context("beta·ΔIWM overflow")?;
    // SCALE_6 · SCALE_12 / SCALE_12 = SCALE_6. Truncating toward zero is
    // fine — any fractional dust below 1 µ-unit is below the minimum
    // hedge trade size anyway.
    let iwm_contribution_s6 = scaled / SCALE_12;
    let iwm_contribution_i64 =
        i64::try_from(iwm_contribution_s6).context("iwm contribution does not fit in i64")?;
    leg_delta_s6
        .checked_add(iwm_contribution_i64)
        .context("leg target overflow")
}

#[derive(Debug, Clone, Copy)]
enum RebalanceDecision {
    Skip,
    TriggeredByCadence,
    TriggeredByBreach,
}

fn rebalance_decision(
    last_rebalance_ts: i64,
    now_ts: i64,
    last_target_spy_s6: i64,
    last_target_qqq_s6: i64,
    new_target_spy_s6: i64,
    new_target_qqq_s6: i64,
    cooldown_secs: i64,
    breach_multiple_bps: u64,
    band_width_bps: u64,
) -> RebalanceDecision {
    let elapsed = now_ts.saturating_sub(last_rebalance_ts);
    if elapsed >= cooldown_secs {
        return RebalanceDecision::TriggeredByCadence;
    }
    let threshold_bps = band_width_bps.saturating_mul(breach_multiple_bps) / 10_000;
    if bps_exceeded(last_target_spy_s6, new_target_spy_s6, threshold_bps)
        || bps_exceeded(last_target_qqq_s6, new_target_qqq_s6, threshold_bps)
    {
        return RebalanceDecision::TriggeredByBreach;
    }
    RebalanceDecision::Skip
}

fn bps_exceeded(previous_s6: i64, current_s6: i64, threshold_bps: u64) -> bool {
    let reference = previous_s6.unsigned_abs().max(1);
    let diff = (i128::from(current_s6) - i128::from(previous_s6)).unsigned_abs();
    let scaled = diff.saturating_mul(10_000);
    let threshold = u128::from(reference).saturating_mul(threshold_bps as u128);
    scaled > threshold
}

#[derive(Debug, Clone, Copy)]
struct PythSnapshot {
    price_s6: i64,
    publish_time: i64,
}

async fn read_pyth(
    rpc: &RpcClient,
    address: &Pubkey,
    feed_id: &[u8; 32],
    now: i64,
    staleness_cap: i64,
) -> Result<PythSnapshot> {
    let account = rpc
        .get_account(address)
        .await
        .with_context(|| format!("fetching Pyth account {address}"))?;
    anyhow::ensure!(
        account.owner == pyth_solana_receiver_sdk::ID,
        "unexpected Pyth owner {}",
        account.owner
    );
    let mut slice: &[u8] = &account.data;
    let update = PriceUpdateV2::try_deserialize(&mut slice)?;
    anyhow::ensure!(
        update.price_message.feed_id == *feed_id,
        "unexpected Pyth feed id"
    );
    anyhow::ensure!(
        matches!(update.verification_level, VerificationLevel::Full),
        "Pyth verification level is not Full"
    );
    anyhow::ensure!(
        now.saturating_sub(update.price_message.publish_time) <= staleness_cap,
        "Pyth price is stale"
    );
    Ok(PythSnapshot {
        price_s6: rescale_to_s6(update.price_message.price, update.price_message.exponent)?,
        publish_time: update.price_message.publish_time,
    })
}

fn rescale_to_s6(value: i64, expo: i32) -> Result<i64> {
    let shift = expo.checked_add(6).context("expo shift overflow")?;
    if shift == 0 {
        return Ok(value);
    }
    if shift > 0 {
        return value
            .checked_mul(pow10_i64(shift as u32)?)
            .context("rescale overflow");
    }
    Ok(value / pow10_i64((-shift) as u32)?)
}

fn pow10_i64(n: u32) -> Result<i64> {
    let mut out = 1i64;
    for _ in 0..n {
        out = out.checked_mul(10).context("pow10 overflow")?;
    }
    Ok(out)
}

fn spot_drift_bps(reference_s6: i64, observed_s6: i64) -> u64 {
    let reference = reference_s6.unsigned_abs().max(1);
    let diff = (i128::from(observed_s6) - i128::from(reference_s6)).unsigned_abs();
    let scaled = diff.saturating_mul(10_000);
    u64::try_from(scaled / u128::from(reference)).unwrap_or(u64::MAX)
}

fn unix_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = Args::parse();
    let cfg = KeeperConfig::load(&args.config)?;
    let client = KeeperClient::connect(&cfg).await?;

    info!(
        target = "flagship_hedge_keeper",
        endpoint = %cfg.rpc_endpoint,
        product = %halcyon_flagship_autocall::ID,
        dry_run = args.dry_run,
        "flagship hedge keeper starting (scaffold — live-submit path deferred)",
    );

    if args.once {
        run_once(&client, &cfg, args.dry_run).await?;
        return Ok(());
    }

    let shutdown = tokio::signal::ctrl_c();
    tokio::select! {
        result = run_forever(&client, &cfg, args.dry_run) => {
            warn!(target = "flagship_hedge_keeper", ?result, "scheduler exited");
            result
        }
        _ = shutdown => {
            info!(target = "flagship_hedge_keeper", "SIGINT received; shutting down");
            Ok(())
        }
    }
}

async fn run_forever(client: &KeeperClient, cfg: &KeeperConfig, dry_run: bool) -> Result<()> {
    let mut consecutive_failures: u32 = 0;
    let mut backoff_secs: u64 = 1;
    loop {
        match run_once(client, cfg, dry_run).await {
            Ok(()) => {
                consecutive_failures = 0;
                backoff_secs = 1;
                sleep(Duration::from_secs(cfg.scan_interval_secs)).await;
            }
            Err(err) => {
                consecutive_failures += 1;
                error!(
                    target = "flagship_hedge_keeper",
                    %err,
                    consecutive_failures,
                    "hedge pass failed"
                );
                if consecutive_failures >= cfg.failure_budget {
                    warn!(
                        target = "flagship_hedge_keeper",
                        failure_budget = cfg.failure_budget,
                        "failure budget exhausted; exiting for ops alert"
                    );
                    return Err(err);
                }
                sleep(Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(cfg.backoff_cap_secs);
            }
        }
    }
}

async fn run_once(client: &KeeperClient, cfg: &KeeperConfig, dry_run: bool) -> Result<()> {
    let now_ts = unix_now();

    // --- Protocol config (for Pyth staleness cap) ---
    let protocol_config = fetch_anchor_account::<halcyon_kernel::state::ProtocolConfig>(
        &client.rpc,
        &pda::protocol_config().0,
    )
    .await?;
    let pyth_staleness_cap = protocol_config.pyth_quote_staleness_cap_secs;

    // --- KeeperRegistry — confirm this keeper is the registered hedge authority ---
    let keeper_registry = fetch_anchor_account::<halcyon_kernel::state::KeeperRegistry>(
        &client.rpc,
        &pda::keeper_registry().0,
    )
    .await?;
    anyhow::ensure!(
        keeper_registry.hedge == client.signer.pubkey(),
        "keeper signer {} is not the registered flagship hedge authority ({})",
        client.signer.pubkey(),
        keeper_registry.hedge,
    );

    // --- AggregateDelta (F2 + F4b + F4a fields included) ---
    let (aggregate_delta_pda, _) = pda::aggregate_delta(&halcyon_flagship_autocall::ID);
    let Some(agg) = fetch_anchor_account_opt::<halcyon_kernel::state::AggregateDelta>(
        &client.rpc,
        &aggregate_delta_pda,
    )
    .await?
    else {
        info!(
            target = "flagship_hedge_keeper",
            "AggregateDelta not yet written for this product; waiting for delta keeper"
        );
        return Ok(());
    };

    // --- F2 propagation: confirm on-chain publish_times are recent ---
    for (label, pt) in ["SPY", "QQQ", "IWM"]
        .iter()
        .zip(agg.pyth_publish_times.iter())
    {
        anyhow::ensure!(
            now_ts.saturating_sub(*pt) <= cfg.aggregate_delta_max_age_secs,
            "AggregateDelta {label} publish_time is stale ({}s)",
            now_ts.saturating_sub(*pt)
        );
    }

    // --- Dual-staleness surface: age of delta write + spot drift ---
    let agg_age = now_ts.saturating_sub(agg.last_update_ts);
    anyhow::ensure!(
        agg_age <= cfg.aggregate_delta_max_age_secs,
        "AggregateDelta age {agg_age}s exceeds configured cap {}s",
        cfg.aggregate_delta_max_age_secs
    );

    let spot_spy = read_pyth(
        &client.rpc,
        &client.pyth_spy,
        &halcyon_oracles::feed_ids::SPY_USD,
        now_ts,
        pyth_staleness_cap,
    )
    .await?;
    let spot_qqq = read_pyth(
        &client.rpc,
        &client.pyth_qqq,
        &halcyon_oracles::feed_ids::QQQ_USD,
        now_ts,
        pyth_staleness_cap,
    )
    .await?;
    let spot_iwm = read_pyth(
        &client.rpc,
        &client.pyth_iwm,
        &halcyon_oracles::feed_ids::IWM_USD,
        now_ts,
        pyth_staleness_cap,
    )
    .await?;

    let drift_spy = spot_drift_bps(agg.spot_spy_s6, spot_spy.price_s6);
    let drift_qqq = spot_drift_bps(agg.spot_qqq_s6, spot_qqq.price_s6);
    let drift_iwm = spot_drift_bps(agg.spot_iwm_s6, spot_iwm.price_s6);
    let worst_drift = drift_spy.max(drift_qqq).max(drift_iwm);
    anyhow::ensure!(
        worst_drift <= cfg.aggregate_delta_max_spot_drift_bps,
        "spot drift against AggregateDelta snapshot exceeds {}bps: SPY={}bps QQQ={}bps IWM={}bps",
        cfg.aggregate_delta_max_spot_drift_bps,
        drift_spy,
        drift_qqq,
        drift_iwm
    );

    // --- Regression (IWM → SPY, QQQ projection) ---
    let (regression_pda, _) = pda::regression();
    let regression =
        fetch_anchor_account::<halcyon_kernel::state::Regression>(&client.rpc, &regression_pda)
            .await?;
    let regression_age = now_ts.saturating_sub(regression.last_update_ts);
    anyhow::ensure!(
        regression_age <= cfg.regression_max_age_secs,
        "Regression age {regression_age}s exceeds configured cap {}s",
        cfg.regression_max_age_secs
    );

    // --- 2D hedge composition ---
    let target = compose_hedge_target_s6(
        agg.delta_spy_s6,
        agg.delta_qqq_s6,
        agg.delta_iwm_s6,
        regression.beta_spy_s12,
        regression.beta_qqq_s12,
    )?;

    // --- HedgeBookState read — current positions and last_rebalance_ts ---
    let (hedge_book_pda, _) = pda::hedge_book(&halcyon_flagship_autocall::ID);
    let hedge_book = fetch_anchor_account_opt::<halcyon_kernel::state::HedgeBookState>(
        &client.rpc,
        &hedge_book_pda,
    )
    .await?;
    let (last_rebalance_ts, last_target_spy_s6, last_target_qqq_s6) = hedge_book
        .as_ref()
        .map(|hb| {
            (
                hb.last_rebalance_ts,
                hb.legs[0].target_position_raw,
                hb.legs[1].target_position_raw,
            )
        })
        .unwrap_or((0, 0, 0));

    let decision = rebalance_decision(
        last_rebalance_ts,
        now_ts,
        last_target_spy_s6,
        last_target_qqq_s6,
        target.target_spy_s6,
        target.target_qqq_s6,
        cfg.rebalance_cooldown_secs,
        cfg.rebalance_breach_multiple_bps,
        cfg.leg_band_width_bps,
    );

    info!(
        target = "flagship_hedge_keeper",
        delta_spy_s6 = agg.delta_spy_s6,
        delta_qqq_s6 = agg.delta_qqq_s6,
        delta_iwm_s6 = agg.delta_iwm_s6,
        beta_spy_s12 = %regression.beta_spy_s12,
        beta_qqq_s12 = %regression.beta_qqq_s12,
        target_spy_s6 = target.target_spy_s6,
        target_qqq_s6 = target.target_qqq_s6,
        last_target_spy_s6,
        last_target_qqq_s6,
        agg_age_secs = agg_age,
        regression_age_secs = regression_age,
        worst_spot_drift_bps = worst_drift,
        decision = ?decision,
        "flagship hedge target composed"
    );

    match decision {
        RebalanceDecision::Skip => {
            info!(
                target = "flagship_hedge_keeper",
                "no rebalance trigger (within cadence cooldown and bands)"
            );
            return Ok(());
        }
        RebalanceDecision::TriggeredByCadence | RebalanceDecision::TriggeredByBreach => {
            // Scaffold pass: no live submission. Log what would have been
            // submitted and exit. The live-submit path will:
            //
            //   1. Read current HedgeBookState leg positions.
            //   2. For each leg where |current - target| exceeds
            //      `leg_band_width_bps × |target|`, compute required trade.
            //   3. Fetch Jupiter route quote, apply JUPITER_PRICE_SANITY_BPS
            //      against Pyth equity spot, build Flash-Fill v0 transaction
            //      with prepare_hedge_swap → Jupiter → record_hedge_trade.
            //   4. Resolve ALTs from RPC (not Jupiter response).
            //   5. Submit, await confirmation, update local state.
            //
            // All of that reuses the SOL Autocall hedge keeper's helpers.
            warn!(
                target = "flagship_hedge_keeper",
                dry_run,
                "rebalance triggered — live-submit path is not implemented in the scaffold; see docs/audit/OPEN_QUESTIONS.md"
            );
            return Ok(());
        }
    }
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt()
        .with_env_filter(filter)
        .json()
        .with_current_span(false)
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition_produces_expected_targets() {
        // Δ_SPY = +100 µ, Δ_QQQ = -50 µ, Δ_IWM = +200 µ
        // β_SPY (s12) = 0.5 · 1e12 = 5e11
        // β_QQQ (s12) = 0.3 · 1e12 = 3e11
        // target_SPY = 100 + 0.5 · 200 = 200
        // target_QQQ = -50 + 0.3 · 200 = 10
        let beta_spy_s12 = 500_000_000_000i128;
        let beta_qqq_s12 = 300_000_000_000i128;
        let t = compose_hedge_target_s6(100, -50, 200, beta_spy_s12, beta_qqq_s12).unwrap();
        assert_eq!(t.target_spy_s6, 200);
        assert_eq!(t.target_qqq_s6, 10);
    }

    #[test]
    fn composition_handles_negative_beta() {
        // Unusual but legal: negative beta for IWM against SPY.
        let t = compose_hedge_target_s6(100, 200, 100, -500_000_000_000i128, 200_000_000_000i128)
            .unwrap();
        assert_eq!(t.target_spy_s6, 100 + (-1 * 50)); // 100 - 50 = 50
        assert_eq!(t.target_qqq_s6, 200 + 20); // 220
    }

    #[test]
    fn composition_truncates_toward_zero() {
        // iwm_delta × beta_s12 / SCALE_12 = 7 · 0.3e12 / 1e12 = 2.1 -> 2
        let t = compose_hedge_target_s6(0, 0, 7, 300_000_000_000i128, 0i128).unwrap();
        assert_eq!(t.target_spy_s6, 2);
    }

    #[test]
    fn rebalance_triggers_on_cooldown_elapsed() {
        let d = rebalance_decision(0, 1_000_000, 0, 0, 0, 0, 5 * 86_400, 15_000, 1_000);
        assert!(matches!(d, RebalanceDecision::TriggeredByCadence));
    }

    #[test]
    fn rebalance_triggers_on_breach_within_cooldown() {
        // breach multiple 1.5×, band 1000 bps → threshold 1500 bps = 15%
        // previous target 1000; current target 1160 -> 16% change, triggers
        let d = rebalance_decision(
            100,
            110, // still inside cooldown
            1000,
            0,
            1160,
            0,
            5 * 86_400,
            15_000,
            1_000,
        );
        assert!(matches!(d, RebalanceDecision::TriggeredByBreach));
    }

    #[test]
    fn rebalance_skips_inside_bands_and_cooldown() {
        // previous 1000, current 1100 = 10% change, threshold 15% → skip
        let d = rebalance_decision(100, 110, 1000, 0, 1100, 0, 5 * 86_400, 15_000, 1_000);
        assert!(matches!(d, RebalanceDecision::Skip));
    }

    #[test]
    fn spot_drift_bps_accurate() {
        assert_eq!(spot_drift_bps(1_000_000, 1_005_000), 50);
        assert_eq!(spot_drift_bps(1_000_000, 995_000), 50);
        assert_eq!(spot_drift_bps(1_000_000, 1_000_000), 0);
    }
}
