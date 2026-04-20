use anchor_lang::AccountDeserialize;
use anyhow::{anyhow, Context, Result};
use clap::Parser;
use halcyon_client_sdk::{decode::fetch_anchor_account_opt, kernel, pda, tx::send_instructions};
use nalgebra::{DMatrix, DVector};
use pyth_solana_receiver_sdk::price_update::{PriceUpdateV2, VerificationLevel};
use serde::Deserialize;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    signer::Signer,
};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use tracing::{error, info, warn};

const WINDOW: usize = 252;
const MIN_REWRITE_GAP_SECS: i64 = 18 * 60 * 60;
const PRICE_SANITY_BPS: u64 = 100;
const MAX_PYTH_CLOCK_SKEW_SECS: i64 = 5;

#[derive(Parser, Debug)]
#[command(
    name = "regression_keeper",
    about = "Halcyon flagship regression keeper"
)]
struct Args {
    #[arg(long, default_value = "config/regression_keeper.json")]
    config: String,

    #[arg(long)]
    once: bool,
}

#[derive(Debug, Deserialize)]
struct KeeperConfig {
    rpc_endpoint: String,
    keypair_path: String,
    pyth_spy: String,
    pyth_qqq: String,
    pyth_iwm: String,
    #[serde(default = "default_spy_history_url")]
    spy_history_url: String,
    #[serde(default = "default_qqq_history_url")]
    qqq_history_url: String,
    #[serde(default = "default_iwm_history_url")]
    iwm_history_url: String,
    #[serde(default = "default_scan_interval_secs")]
    scan_interval_secs: u64,
    #[serde(default = "default_backoff_cap_secs")]
    backoff_cap_secs: u64,
    #[serde(default = "default_failure_budget")]
    failure_budget: u32,
}

fn default_spy_history_url() -> String {
    "https://stooq.com/q/d/l/?s=spy.us&i=d".to_string()
}

fn default_qqq_history_url() -> String {
    "https://stooq.com/q/d/l/?s=qqq.us&i=d".to_string()
}

fn default_iwm_history_url() -> String {
    "https://stooq.com/q/d/l/?s=iwm.us&i=d".to_string()
}

fn default_scan_interval_secs() -> u64 {
    24 * 60 * 60
}

fn default_backoff_cap_secs() -> u64 {
    15 * 60
}

fn default_failure_budget() -> u32 {
    5
}

impl KeeperConfig {
    fn load(path: &str) -> Result<Self> {
        let raw = std::fs::read_to_string(Path::new(path))
            .with_context(|| format!("reading regression-keeper config at {path}"))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("parsing regression-keeper config at {path}"))
    }

    fn load_keypair(&self) -> Result<Keypair> {
        solana_sdk::signer::keypair::read_keypair_file(&self.keypair_path)
            .map_err(|e| anyhow!("reading keypair at {}: {}", self.keypair_path, e))
    }
}

struct KeeperClient {
    rpc: Arc<RpcClient>,
    http: reqwest::Client,
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
            http: reqwest::Client::builder()
                .build()
                .context("building HTTP client")?,
            signer: cfg.load_keypair()?,
            pyth_spy: Pubkey::from_str(&cfg.pyth_spy)
                .with_context(|| format!("parsing pyth_spy {}", cfg.pyth_spy))?,
            pyth_qqq: Pubkey::from_str(&cfg.pyth_qqq)
                .with_context(|| format!("parsing pyth_qqq {}", cfg.pyth_qqq))?,
            pyth_iwm: Pubkey::from_str(&cfg.pyth_iwm)
                .with_context(|| format!("parsing pyth_iwm {}", cfg.pyth_iwm))?,
        })
    }

    async fn send_write_regression(
        &self,
        args: halcyon_kernel::WriteRegressionArgs,
    ) -> Result<Signature> {
        let ix = kernel::write_regression_ix(&self.signer.pubkey(), &self.signer.pubkey(), args);
        send_instructions(&self.rpc, &self.signer, vec![ix]).await
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = Args::parse();
    let cfg = KeeperConfig::load(&args.config)?;
    let client = KeeperClient::connect(&cfg).await?;

    info!(
        target = "regression_keeper",
        endpoint = %cfg.rpc_endpoint,
        product = %halcyon_flagship_autocall::ID,
        "regression keeper starting",
    );

    if args.once {
        run_once(&client, &cfg).await?;
        return Ok(());
    }

    let shutdown = tokio::signal::ctrl_c();
    tokio::select! {
        result = run_forever(&client, &cfg) => {
            warn!(target = "regression_keeper", ?result, "scheduler exited");
            result
        }
        _ = shutdown => {
            info!(target = "regression_keeper", "SIGINT received; shutting down");
            Ok(())
        }
    }
}

async fn run_forever(client: &KeeperClient, cfg: &KeeperConfig) -> Result<()> {
    let mut consecutive_failures: u32 = 0;
    let mut backoff_secs: u64 = 1;

    loop {
        match run_once(client, cfg).await {
            Ok(()) => {
                consecutive_failures = 0;
                backoff_secs = 1;
                sleep(Duration::from_secs(cfg.scan_interval_secs)).await;
            }
            Err(err) => {
                consecutive_failures += 1;
                error!(
                    target = "regression_keeper",
                    %err,
                    consecutive_failures,
                    "regression pass failed",
                );
                if consecutive_failures >= cfg.failure_budget {
                    warn!(
                        target = "regression_keeper",
                        failure_budget = cfg.failure_budget,
                        "failure budget exhausted; exiting for ops alert",
                    );
                    return Err(err);
                }
                sleep(Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(cfg.backoff_cap_secs);
            }
        }
    }
}

async fn run_once(client: &KeeperClient, cfg: &KeeperConfig) -> Result<()> {
    let now = unix_now();
    let protocol_config = fetch_anchor_account_opt::<halcyon_kernel::state::ProtocolConfig>(
        &client.rpc,
        &pda::protocol_config().0,
    )
    .await?
    .context("missing protocol config")?;
    if let Some(regression) = fetch_anchor_account_opt::<halcyon_kernel::state::Regression>(
        &client.rpc,
        &pda::regression().0,
    )
    .await?
    {
        let age = now.saturating_sub(regression.last_update_ts);
        if age < MIN_REWRITE_GAP_SECS {
            info!(
                target = "regression_keeper",
                age_secs = age,
                min_gap_secs = MIN_REWRITE_GAP_SECS,
                "skipping regression write; previous regression is still within the minimum gap",
            );
            return Ok(());
        }
    }

    let spy_closes = fetch_close_series(&client.http, &cfg.spy_history_url).await?;
    let qqq_closes = fetch_close_series(&client.http, &cfg.qqq_history_url).await?;
    let iwm_closes = fetch_close_series(&client.http, &cfg.iwm_history_url).await?;

    let latest_spy_close = *spy_closes.last().context("missing latest SPY close")?;
    let latest_qqq_close = *qqq_closes.last().context("missing latest QQQ close")?;
    let latest_iwm_close = *iwm_closes.last().context("missing latest IWM close")?;

    let spot_spy_s6 = read_pyth_price_s6(
        &client.rpc,
        &client.pyth_spy,
        &halcyon_oracles::feed_ids::SPY_USD,
        now,
        protocol_config.pyth_quote_staleness_cap_secs,
    )
    .await?;
    let spot_qqq_s6 = read_pyth_price_s6(
        &client.rpc,
        &client.pyth_qqq,
        &halcyon_oracles::feed_ids::QQQ_USD,
        now,
        protocol_config.pyth_quote_staleness_cap_secs,
    )
    .await?;
    let spot_iwm_s6 = read_pyth_price_s6(
        &client.rpc,
        &client.pyth_iwm,
        &halcyon_oracles::feed_ids::IWM_USD,
        now,
        protocol_config.pyth_quote_staleness_cap_secs,
    )
    .await?;

    ensure_price_sanity("SPY", latest_spy_close, spot_spy_s6)?;
    ensure_price_sanity("QQQ", latest_qqq_close, spot_qqq_s6)?;
    ensure_price_sanity("IWM", latest_iwm_close, spot_iwm_s6)?;

    let spy_returns = log_returns(&spy_closes)?;
    let qqq_returns = log_returns(&qqq_closes)?;
    let iwm_returns = log_returns(&iwm_closes)?;
    let regression = solve_regression(&spy_returns, &qqq_returns, &iwm_returns)?;

    let sig = client
        .send_write_regression(halcyon_kernel::WriteRegressionArgs {
            beta_spy_s12: scale_to_s12(regression.beta_spy)?,
            beta_qqq_s12: scale_to_s12(regression.beta_qqq)?,
            alpha_s12: scale_to_s12(regression.alpha)?,
            r_squared_s6: scale_to_s6(regression.r_squared)?,
            residual_vol_s6: scale_to_s6(regression.residual_vol_annualised)?,
            window_start_ts: now.saturating_sub((WINDOW as i64) * 86_400),
            window_end_ts: now,
            sample_count: u32::try_from(regression.sample_count)
                .context("sample_count overflow")?,
        })
        .await?;

    info!(
        target = "regression_keeper",
        beta_spy = regression.beta_spy,
        beta_qqq = regression.beta_qqq,
        alpha = regression.alpha,
        r_squared = regression.r_squared,
        residual_vol = regression.residual_vol_annualised,
        sample_count = regression.sample_count,
        %sig,
        "wrote flagship regression state",
    );
    Ok(())
}

#[derive(Debug)]
struct RegressionSnapshot {
    alpha: f64,
    beta_spy: f64,
    beta_qqq: f64,
    r_squared: f64,
    residual_vol_annualised: f64,
    sample_count: usize,
}

fn solve_regression(spy: &[f64], qqq: &[f64], iwm: &[f64]) -> Result<RegressionSnapshot> {
    let n = spy.len().min(qqq.len()).min(iwm.len());
    anyhow::ensure!(
        n >= WINDOW,
        "insufficient overlapping return samples for regression"
    );
    let spy = &spy[spy.len() - WINDOW..];
    let qqq = &qqq[qqq.len() - WINDOW..];
    let iwm = &iwm[iwm.len() - WINDOW..];

    let mut x = DMatrix::<f64>::zeros(WINDOW, 3);
    let mut y = DVector::<f64>::zeros(WINDOW);
    for row in 0..WINDOW {
        x[(row, 0)] = 1.0;
        x[(row, 1)] = spy[row];
        x[(row, 2)] = qqq[row];
        y[row] = iwm[row];
    }

    let xtx = x.transpose() * &x;
    let xty = x.transpose() * &y;
    let Some(beta) = xtx.lu().solve(&xty) else {
        return Err(anyhow!("regression matrix is singular"));
    };
    let fitted = &x * &beta;
    let mut sse = 0.0f64;
    let mut mean_y = 0.0f64;
    for value in iwm {
        mean_y += *value;
    }
    mean_y /= WINDOW as f64;
    let mut sst = 0.0f64;
    for row in 0..WINDOW {
        let resid = y[row] - fitted[row];
        sse += resid * resid;
        let centered = y[row] - mean_y;
        sst += centered * centered;
    }
    let dof = (WINDOW as f64 - 3.0).max(1.0);
    let residual_std = (sse / dof).sqrt();
    let residual_vol_annualised = residual_std * (252.0f64).sqrt();
    let r_squared = if sst > 0.0 { 1.0 - (sse / sst) } else { 0.0 };

    Ok(RegressionSnapshot {
        alpha: beta[0],
        beta_spy: beta[1],
        beta_qqq: beta[2],
        r_squared,
        residual_vol_annualised,
        sample_count: WINDOW,
    })
}

async fn fetch_close_series(client: &reqwest::Client, url: &str) -> Result<Vec<f64>> {
    let body = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("fetching historical prices from {url}"))?
        .error_for_status()
        .with_context(|| format!("historical price response status for {url}"))?
        .text()
        .await
        .with_context(|| format!("reading historical price body from {url}"))?;
    parse_stooq_csv(&body)
}

fn parse_stooq_csv(raw: &str) -> Result<Vec<f64>> {
    let mut closes = Vec::new();
    for (idx, line) in raw.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let mut cols = line.split(',');
        let _date = cols.next();
        let _open = cols.next();
        let _high = cols.next();
        let _low = cols.next();
        let close = cols
            .next()
            .context("missing close column in CSV")?
            .trim()
            .parse::<f64>()
            .context("parsing close price")?;
        if close.is_finite() && close > 0.0 {
            closes.push(close);
        }
    }
    anyhow::ensure!(
        closes.len() > WINDOW,
        "not enough closes in historical series"
    );
    Ok(closes)
}

fn log_returns(closes: &[f64]) -> Result<Vec<f64>> {
    anyhow::ensure!(closes.len() > WINDOW, "not enough closes for returns");
    let mut out = Vec::with_capacity(closes.len().saturating_sub(1));
    for pair in closes.windows(2) {
        let prev = pair[0];
        let next = pair[1];
        anyhow::ensure!(prev > 0.0 && next > 0.0, "close prices must be positive");
        out.push((next / prev).ln());
    }
    Ok(out)
}

async fn read_pyth_price_s6(
    rpc: &RpcClient,
    address: &Pubkey,
    feed_id: &[u8; 32],
    now: i64,
    staleness_cap_secs: i64,
) -> Result<i64> {
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
        "unexpected feed id"
    );
    anyhow::ensure!(
        matches!(update.verification_level, VerificationLevel::Full),
        "Pyth verification level is not Full"
    );
    validate_publish_time(now, update.price_message.publish_time, staleness_cap_secs)?;
    rescale_to_s6(update.price_message.price, update.price_message.exponent)
}

fn validate_publish_time(now: i64, publish_time: i64, staleness_cap_secs: i64) -> Result<()> {
    anyhow::ensure!(staleness_cap_secs > 0, "invalid Pyth staleness cap");
    anyhow::ensure!(publish_time > 0, "invalid Pyth publish time");
    anyhow::ensure!(
        publish_time <= now.saturating_add(MAX_PYTH_CLOCK_SKEW_SECS),
        "Pyth publish time is in the future"
    );
    anyhow::ensure!(
        now.saturating_sub(publish_time) <= staleness_cap_secs,
        "Pyth price is stale"
    );
    Ok(())
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

fn ensure_price_sanity(symbol: &str, latest_close: f64, spot_s6: i64) -> Result<()> {
    let latest_close_s6 = scale_to_s6(latest_close)?;
    let diff = (i128::from(latest_close_s6) - i128::from(spot_s6)).abs() as u128;
    let deviation_bps = diff.checked_mul(10_000).context("price sanity overflow")?
        / u128::try_from(latest_close_s6).context("negative latest close")?;
    anyhow::ensure!(
        deviation_bps <= PRICE_SANITY_BPS as u128,
        "{symbol} latest close diverges from Pyth by {deviation_bps} bps"
    );
    Ok(())
}

fn scale_to_s6(value: f64) -> Result<i64> {
    anyhow::ensure!(value.is_finite(), "value must be finite");
    Ok((value * 1_000_000.0).round() as i64)
}

fn scale_to_s12(value: f64) -> Result<i128> {
    anyhow::ensure!(value.is_finite(), "value must be finite");
    Ok((value * 1_000_000_000_000.0).round() as i128)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
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
    use super::validate_publish_time;

    #[test]
    fn rejects_stale_publish_time() {
        let err = validate_publish_time(1_000, 900, 30).unwrap_err();
        assert!(err.to_string().contains("stale"));
    }

    #[test]
    fn accepts_recent_publish_time_with_small_clock_skew() {
        validate_publish_time(1_000, 1_002, 30).unwrap();
    }
}
