use anchor_lang::AccountDeserialize;
use anyhow::{anyhow, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use nalgebra::{DMatrix, DVector};
use pyth_solana_receiver_sdk::price_update::{PriceUpdateV2, VerificationLevel};
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signer::Signer;
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use halcyon_client_sdk::{
    decode::{fetch_anchor_account, fetch_anchor_account_opt},
    kernel, pda, sol_autocall, tx,
};
use halcyon_sol_autocall_quote::{
    autocall_v2::AutocallParams,
    autocall_v2_e11::precompute_reduced_operators_from_const,
    generated::pod_deim_table::{
        TRAINING_ALPHA_S6, TRAINING_AUTOCALL_LOG_6, TRAINING_BETA_S6, TRAINING_KNOCK_IN_LOG_6,
        TRAINING_NO_AUTOCALL_FIRST_N_OBS, TRAINING_N_OBS, TRAINING_REFERENCE_STEP_DAYS,
    },
};
use solana_sdk::pubkey::Pubkey;

use crate::client::CliContext;

const REDUCED_OPERATOR_CHUNK_LEN: usize = 48;
const REGRESSION_WINDOW: usize = 252;
const REGRESSION_PRICE_SANITY_BPS: u64 = 100;
const MAX_PYTH_CLOCK_SKEW_SECS: i64 = 5;
const MIN_REGRESSION_SAMPLE_COUNT: u32 = 60;
const MIN_REGRESSION_BETA_S12: i128 = -2_000_000_000_000;
const MAX_REGRESSION_BETA_S12: i128 = 3_000_000_000_000;
const MAX_ABS_REGRESSION_ALPHA_S12: i128 = 5_000_000_000;
const MAX_REGRESSION_R_SQUARED_S6: i64 = 1_000_000;
const MAX_HISTORY_END_DATE_SKEW_SECS: i64 = 7 * 86_400;

fn default_spy_history_source() -> String {
    default_history_source("spy_1d.csv")
}

fn default_qqq_history_source() -> String {
    default_history_source("qqq_1d.csv")
}

fn default_iwm_history_source() -> String {
    default_history_source("iwm_1d.csv")
}

fn default_history_source(file_name: &str) -> String {
    let candidates = default_history_source_candidates(file_name);
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned()
        .unwrap_or_else(|| candidates[0].clone())
        .display()
        .to_string()
}

fn default_history_source_candidates(file_name: &str) -> Vec<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .ancestors()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or(manifest_dir);
    ["Colosseum", "colosseum"]
        .into_iter()
        .map(|repo| {
            repo_root
                .join("..")
                .join(repo)
                .join("halcyon-hedge-lab")
                .join("data")
                .join("cache")
                .join(file_name)
        })
        .collect()
}

#[derive(Debug, Subcommand)]
pub enum KeeperCmd {
    FireObservation(FireObservationArgs),
    FireHedge(FireHedgeArgs),
    FireRegime(FireRegimeArgs),
    FireReducedOps(FireReducedOpsArgs),
    WriteRegression(WriteRegressionArgs),
    WriteSigmaValue(WriteSigmaValueArgs),
    WriteAutocallSchedule(WriteAutocallScheduleArgs),
}

#[derive(Debug, ClapArgs)]
pub struct FireObservationArgs {
    pub policy: String,
    #[arg(long)]
    pub usdc_mint: String,
    #[arg(long)]
    pub pyth_sol: String,
}

#[derive(Debug, ClapArgs)]
pub struct FireHedgeArgs {
    #[arg(long, default_value = "SOL")]
    pub asset_tag: String,
    #[arg(long, default_value_t = 0)]
    pub leg_index: u8,
    #[arg(long)]
    pub new_position_raw: i64,
    #[arg(long)]
    pub executed_price_s6: i64,
    #[arg(long, default_value_t = 0)]
    pub execution_cost: u64,
    #[arg(long)]
    pub trade_delta_raw: Option<i64>,
    #[arg(long)]
    pub sequence: Option<u64>,
}

#[derive(Debug, ClapArgs)]
pub struct FireRegimeArgs {
    #[arg(
        long,
        default_value = "https://api.coingecko.com/api/v3/coins/solana/market_chart?vs_currency=usd&days=120&interval=daily"
    )]
    pub history_url: String,
    #[arg(long)]
    pub fvol_s6: Option<i64>,
    /// Product that owns the regime_signal PDA to seed. One of `il`, `sol`, `flagship`.
    #[arg(long, default_value = "il")]
    pub product: String,
}

#[derive(Debug, ClapArgs)]
pub struct FireReducedOpsArgs {}

#[derive(Debug, ClapArgs)]
pub struct WriteRegressionArgs {
    #[arg(long)]
    pub pyth_spy: Option<String>,
    #[arg(long)]
    pub pyth_qqq: Option<String>,
    #[arg(long)]
    pub pyth_iwm: Option<String>,
    #[arg(
        long = "spy-history-source",
        alias = "spy-history-url",
        default_value_t = default_spy_history_source()
    )]
    pub spy_history_source: String,
    #[arg(
        long = "qqq-history-source",
        alias = "qqq-history-url",
        default_value_t = default_qqq_history_source()
    )]
    pub qqq_history_source: String,
    #[arg(
        long = "iwm-history-source",
        alias = "iwm-history-url",
        default_value_t = default_iwm_history_source()
    )]
    pub iwm_history_source: String,
}

#[derive(Debug, ClapArgs)]
pub struct WriteSigmaValueArgs {
    #[arg(long)]
    pub sigma_annualised_s6: i64,
    #[arg(long)]
    pub publish_ts: Option<i64>,
    #[arg(long)]
    pub publish_slot: Option<u64>,
}

#[derive(Debug, ClapArgs)]
pub struct WriteAutocallScheduleArgs {
    #[arg(long)]
    pub issued_at_ts: Option<i64>,
}

pub async fn run(ctx: &CliContext, cmd: KeeperCmd) -> Result<()> {
    match cmd {
        KeeperCmd::FireObservation(a) => fire_observation(ctx, a).await,
        KeeperCmd::FireHedge(a) => fire_hedge(ctx, a).await,
        KeeperCmd::FireRegime(a) => fire_regime(ctx, a).await,
        KeeperCmd::FireReducedOps(a) => fire_reduced_ops(ctx, a).await,
        KeeperCmd::WriteRegression(a) => write_regression(ctx, a).await,
        KeeperCmd::WriteSigmaValue(a) => write_sigma_value(ctx, a).await,
        KeeperCmd::WriteAutocallSchedule(a) => write_autocall_schedule(ctx, a).await,
    }
}

async fn fire_observation(ctx: &CliContext, args: FireObservationArgs) -> Result<()> {
    let keeper = ctx.signer()?;
    let policy = CliContext::parse_pubkey("policy", &args.policy)?;
    let usdc_mint = CliContext::parse_pubkey("usdc_mint", &args.usdc_mint)?;
    let pyth_sol = CliContext::parse_pubkey("pyth_sol", &args.pyth_sol)?;
    let header =
        fetch_anchor_account::<halcyon_kernel::state::PolicyHeader>(ctx.rpc.as_ref(), &policy)
            .await?;
    let terms = fetch_anchor_account::<halcyon_sol_autocall::state::SolAutocallTerms>(
        ctx.rpc.as_ref(),
        &header.product_terms,
    )
    .await?;
    let ix = sol_autocall::record_observation_ix(
        &keeper.pubkey(),
        &usdc_mint,
        pyth_sol,
        &header,
        policy,
        terms.current_observation_index,
    );
    let signature = tx::send_instructions(ctx.rpc.as_ref(), keeper, vec![ix]).await?;
    println!(
        "keepers fire-observation: signature={signature} policy={policy} expected_index={}",
        terms.current_observation_index
    );
    Ok(())
}

async fn fire_hedge(ctx: &CliContext, args: FireHedgeArgs) -> Result<()> {
    let _ = ctx;
    let _ = args;
    anyhow::bail!(
        "keepers fire-hedge is disabled; manual hedge recording was retired. Use the hedge keeper prepare_hedge_swap -> Jupiter swap -> record_hedge_trade flow instead."
    )
}

fn resolve_regime_product(alias: &str) -> Result<Pubkey> {
    match alias.to_ascii_lowercase().as_str() {
        "il" | "il_protection" | "il-protection" => Ok(halcyon_il_protection::ID),
        "sol" | "sol_autocall" | "sol-autocall" => Ok(halcyon_sol_autocall::ID),
        "flagship" | "flagship_autocall" | "flagship-autocall" => Ok(halcyon_flagship_autocall::ID),
        other => anyhow::bail!("unknown --product '{other}' (expected one of: il, sol, flagship)"),
    }
}

async fn fire_regime(ctx: &CliContext, args: FireRegimeArgs) -> Result<()> {
    let keeper = ctx.signer()?;
    let product_program_id = resolve_regime_product(&args.product)?;
    let fvol_s6 = match args.fvol_s6 {
        Some(value) => value,
        None => fetch_fvol_s6(&args.history_url).await?,
    };
    let regime = halcyon_il_quote::classify_regime_from_fvol_s6(fvol_s6);
    let ix = kernel::write_regime_signal_ix(
        &keeper.pubkey(),
        &keeper.pubkey(),
        &product_program_id,
        halcyon_kernel::WriteRegimeSignalArgs {
            product_program_id,
            fvol_s6,
        },
    );
    let signature = tx::send_instructions(ctx.rpc.as_ref(), keeper, vec![ix]).await?;
    println!(
        "keepers fire-regime: signature={signature} product={product_program_id} fvol_s6={fvol_s6} regime={:?} sigma_multiplier_s6={}",
        regime.regime, regime.sigma_multiplier_s6
    );
    Ok(())
}

async fn fire_reduced_ops(ctx: &CliContext, _args: FireReducedOpsArgs) -> Result<()> {
    let keeper = ctx.signer()?;
    let (protocol_config, _) = halcyon_client_sdk::pda::protocol_config();
    let (vault_sigma, _) = halcyon_client_sdk::pda::vault_sigma(&halcyon_sol_autocall::ID);
    let (regime_signal, _) = halcyon_client_sdk::pda::regime_signal(&halcyon_sol_autocall::ID);

    let protocol = fetch_anchor_account::<halcyon_kernel::state::ProtocolConfig>(
        ctx.rpc.as_ref(),
        &protocol_config,
    )
    .await?;
    let sigma =
        fetch_anchor_account::<halcyon_kernel::state::VaultSigma>(ctx.rpc.as_ref(), &vault_sigma)
            .await?;
    let regime = fetch_anchor_account::<halcyon_kernel::state::RegimeSignal>(
        ctx.rpc.as_ref(),
        &regime_signal,
    )
    .await?;

    let sigma_ann_s6 = halcyon_sol_autocall::pricing::compose_pricing_sigma(
        &sigma,
        &regime,
        halcyon_sol_autocall::pricing::protocol_sigma_floor_annualised_s6(&protocol),
    )?;

    let contract = AutocallParams {
        n_obs: TRAINING_N_OBS,
        knock_in_log_6: TRAINING_KNOCK_IN_LOG_6,
        autocall_log_6: TRAINING_AUTOCALL_LOG_6,
        no_autocall_first_n_obs: TRAINING_NO_AUTOCALL_FIRST_N_OBS,
    };
    let reduced = precompute_reduced_operators_from_const(
        sigma_ann_s6,
        TRAINING_ALPHA_S6,
        TRAINING_BETA_S6,
        TRAINING_REFERENCE_STEP_DAYS,
        &contract,
    )
    .map_err(|err| anyhow!("failed to precompute reduced operators: {err:?}"))?;

    let p_red_v = reduced.p_red_v;
    let p_red_u = reduced.p_red_u;
    for (side, values) in [
        (
            halcyon_sol_autocall::ReducedOperatorSide::V,
            p_red_v.as_slice(),
        ),
        (
            halcyon_sol_autocall::ReducedOperatorSide::U,
            p_red_u.as_slice(),
        ),
    ] {
        for start in (0..values.len()).step_by(REDUCED_OPERATOR_CHUNK_LEN) {
            let end = (start + REDUCED_OPERATOR_CHUNK_LEN).min(values.len());
            let ix = sol_autocall::write_reduced_operators_ix(
                &keeper.pubkey(),
                halcyon_sol_autocall::WriteReducedOperatorsArgs {
                    begin_upload: matches!(side, halcyon_sol_autocall::ReducedOperatorSide::V)
                        && start == 0,
                    side,
                    start: start as u16,
                    values: values[start..end].to_vec(),
                },
            );
            let signature = tx::send_instructions(ctx.rpc.as_ref(), keeper, vec![ix]).await?;
            println!(
                "keepers fire-reduced-ops: signature={signature} side={side:?} range={start}..{end} sigma_ann_s6={sigma_ann_s6} vault_sigma_slot={} regime_signal_slot={}",
                sigma.last_update_slot, regime.last_update_slot,
            );
        }
    }
    Ok(())
}

async fn write_regression(ctx: &CliContext, args: WriteRegressionArgs) -> Result<()> {
    let keeper = ctx.signer()?;
    let http = reqwest::Client::builder()
        .build()
        .context("building HTTP client")?;
    let now = unix_now();
    let (regression_pda, _) = pda::regression();

    let existing_regression = fetch_anchor_account_opt::<halcyon_kernel::state::Regression>(
        ctx.rpc.as_ref(),
        &regression_pda,
    )
    .await?;

    let spy_history = load_close_series(&http, &args.spy_history_source).await?;
    let qqq_history = load_close_series(&http, &args.qqq_history_source).await?;
    let iwm_history = load_close_series(&http, &args.iwm_history_source).await?;
    let window_end_ts = regression_window_end_ts(&spy_history, &qqq_history, &iwm_history)?;

    match (&args.pyth_spy, &args.pyth_qqq, &args.pyth_iwm) {
        (Some(pyth_spy), Some(pyth_qqq), Some(pyth_iwm)) => {
            let (protocol_config, _) = pda::protocol_config();
            let protocol = fetch_anchor_account::<halcyon_kernel::state::ProtocolConfig>(
                ctx.rpc.as_ref(),
                &protocol_config,
            )
            .await?;
            let pyth_spy = CliContext::parse_pubkey("pyth_spy", pyth_spy)?;
            let pyth_qqq = CliContext::parse_pubkey("pyth_qqq", pyth_qqq)?;
            let pyth_iwm = CliContext::parse_pubkey("pyth_iwm", pyth_iwm)?;

            let latest_spy_close = *spy_history
                .closes
                .last()
                .context("missing latest SPY close")?;
            let latest_qqq_close = *qqq_history
                .closes
                .last()
                .context("missing latest QQQ close")?;
            let latest_iwm_close = *iwm_history
                .closes
                .last()
                .context("missing latest IWM close")?;

            let spot_spy_s6 = read_pyth_price_s6(
                ctx.rpc.as_ref(),
                &pyth_spy,
                &halcyon_oracles::feed_ids::SPY_USD,
                now,
                protocol.pyth_quote_staleness_cap_secs,
            )
            .await?;
            let spot_qqq_s6 = read_pyth_price_s6(
                ctx.rpc.as_ref(),
                &pyth_qqq,
                &halcyon_oracles::feed_ids::QQQ_USD,
                now,
                protocol.pyth_quote_staleness_cap_secs,
            )
            .await?;
            let spot_iwm_s6 = read_pyth_price_s6(
                ctx.rpc.as_ref(),
                &pyth_iwm,
                &halcyon_oracles::feed_ids::IWM_USD,
                now,
                protocol.pyth_quote_staleness_cap_secs,
            )
            .await?;

            ensure_price_sanity("SPY", latest_spy_close, spot_spy_s6)?;
            ensure_price_sanity("QQQ", latest_qqq_close, spot_qqq_s6)?;
            ensure_price_sanity("IWM", latest_iwm_close, spot_iwm_s6)?;
        }
        (None, None, None) => {}
        _ => anyhow::bail!("pass either all of --pyth-spy/--pyth-qqq/--pyth-iwm or none of them"),
    }

    let spy_returns = log_returns(&spy_history.closes)?;
    let qqq_returns = log_returns(&qqq_history.closes)?;
    let iwm_returns = log_returns(&iwm_history.closes)?;
    let regression = solve_regression(&spy_returns, &qqq_returns, &iwm_returns)?;
    let ix_args = build_write_regression_args(&regression, window_end_ts)?;
    validate_write_regression_args(&ix_args)?;

    let ix = kernel::write_regression_ix(&keeper.pubkey(), &keeper.pubkey(), ix_args.clone());
    let signature = tx::send_instructions(ctx.rpc.as_ref(), keeper, vec![ix]).await?;
    println!(
        "keepers write-regression: signature={signature} mode={} beta_spy_s12={} beta_qqq_s12={} alpha_s12={} r_squared_s6={} residual_vol_s6={} sample_count={} window_end_ts={}",
        if existing_regression.is_some() { "refresh" } else { "bootstrap" },
        ix_args.beta_spy_s12,
        ix_args.beta_qqq_s12,
        ix_args.alpha_s12,
        ix_args.r_squared_s6,
        ix_args.residual_vol_s6,
        ix_args.sample_count,
        ix_args.window_end_ts,
    );
    Ok(())
}

async fn write_sigma_value(ctx: &CliContext, args: WriteSigmaValueArgs) -> Result<()> {
    let keeper = ctx.signer()?;
    let publish_ts = args.publish_ts.unwrap_or_else(unix_now);
    let publish_slot = match args.publish_slot {
        Some(slot) => slot,
        None => ctx.rpc.get_slot().await?,
    };
    let ix_args = halcyon_kernel::WriteSigmaValueArgs {
        sigma_annualised_s6: args.sigma_annualised_s6,
        publish_ts,
        publish_slot,
    };
    let ix = kernel::write_sigma_value_ix(&keeper.pubkey(), ix_args);
    let signature = tx::send_instructions(ctx.rpc.as_ref(), keeper, vec![ix]).await?;
    println!(
        "keepers write-sigma-value: signature={signature} product={} sigma_annualised_s6={} publish_ts={} publish_slot={}",
        halcyon_flagship_autocall::ID,
        args.sigma_annualised_s6,
        publish_ts,
        publish_slot,
    );
    Ok(())
}

async fn write_autocall_schedule(ctx: &CliContext, args: WriteAutocallScheduleArgs) -> Result<()> {
    let keeper = ctx.signer()?;
    let issued_at_ts = args.issued_at_ts.unwrap_or_else(unix_now);
    let (autocall_schedule_pda, _) = pda::autocall_schedule(&halcyon_flagship_autocall::ID);
    let existing_schedule = fetch_anchor_account_opt::<halcyon_kernel::state::AutocallSchedule>(
        ctx.rpc.as_ref(),
        &autocall_schedule_pda,
    )
    .await?;
    let observation_timestamps =
        halcyon_flagship_autocall::pricing::build_quarterly_autocall_schedule_from_calendar(
            issued_at_ts,
        )?;
    let ix_args = halcyon_kernel::WriteAutocallScheduleArgs {
        product_program_id: halcyon_flagship_autocall::ID,
        issue_date_ts: issued_at_ts,
        observation_timestamps,
    };
    let ix = kernel::write_autocall_schedule_ix(&keeper.pubkey(), &keeper.pubkey(), ix_args);
    let signature = tx::send_instructions(ctx.rpc.as_ref(), keeper, vec![ix]).await?;
    println!(
        "keepers write-autocall-schedule: signature={signature} mode={} product={} issue_date_ts={} first_observation_ts={} last_observation_ts={}",
        if existing_schedule.is_some() { "refresh" } else { "bootstrap" },
        halcyon_flagship_autocall::ID,
        issued_at_ts,
        observation_timestamps[0],
        observation_timestamps[5],
    );
    Ok(())
}

#[derive(serde::Deserialize)]
struct CoinGeckoMarketChart {
    prices: Vec<(f64, f64)>,
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

async fn fetch_fvol_s6(history_url: &str) -> Result<i64> {
    let response = reqwest::get(history_url).await?.error_for_status()?;
    let chart: CoinGeckoMarketChart = response.json().await?;
    let closes = chart
        .prices
        .into_iter()
        .map(|(_, price)| price)
        .collect::<Vec<_>>();
    let fvol = halcyon_il_quote::compute_fvol_from_daily_closes(&closes)
        .ok_or_else(|| anyhow::anyhow!("insufficient or invalid price history for fvol"))?;
    Ok((fvol * 1_000_000.0).round() as i64)
}

#[derive(Debug)]
struct HistoricalSeries {
    closes: Vec<f64>,
    last_close_ts: i64,
}

async fn load_close_series(client: &reqwest::Client, source: &str) -> Result<HistoricalSeries> {
    let body = if source.starts_with("http://") || source.starts_with("https://") {
        client
            .get(source)
            .send()
            .await
            .with_context(|| format!("fetching historical prices from {source}"))?
            .error_for_status()
            .with_context(|| format!("historical price response status for {source}"))?
            .text()
            .await
            .with_context(|| format!("reading historical price body from {source}"))?
    } else {
        std::fs::read_to_string(source)
            .with_context(|| format!("reading historical prices from {source}"))?
    };
    parse_history_csv(&body)
}

fn parse_history_csv(raw: &str) -> Result<HistoricalSeries> {
    let mut closes = Vec::new();
    let mut last_close_ts = None;
    for (idx, line) in raw.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let mut cols = line.split(',');
        let date = cols.next().context("missing date column in CSV")?.trim();
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
            last_close_ts = Some(parse_history_date_to_unix_ts(date)?);
        }
    }
    anyhow::ensure!(
        closes.len() > REGRESSION_WINDOW,
        "not enough closes in historical series"
    );
    Ok(HistoricalSeries {
        closes,
        last_close_ts: last_close_ts.context("missing history end date")?,
    })
}

fn parse_history_date_to_unix_ts(raw: &str) -> Result<i64> {
    let date = raw
        .get(..10)
        .context("history date must begin with YYYY-MM-DD")?;
    let bytes = date.as_bytes();
    anyhow::ensure!(
        bytes.len() == 10 && bytes[4] == b'-' && bytes[7] == b'-',
        "history date must begin with YYYY-MM-DD"
    );
    let year = date[0..4].parse::<i32>().context("parsing history year")?;
    let month = date[5..7].parse::<u32>().context("parsing history month")?;
    let day = date[8..10].parse::<u32>().context("parsing history day")?;
    anyhow::ensure!((1..=12).contains(&month), "history month out of range");
    anyhow::ensure!((1..=31).contains(&day), "history day out of range");

    let month = month as i64;
    let day = day as i64;
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * month_prime + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Ok((era * 146_097 + doe - 719_468) * 86_400)
}

fn regression_window_end_ts(
    spy: &HistoricalSeries,
    qqq: &HistoricalSeries,
    iwm: &HistoricalSeries,
) -> Result<i64> {
    let min_end = spy
        .last_close_ts
        .min(qqq.last_close_ts)
        .min(iwm.last_close_ts);
    let max_end = spy
        .last_close_ts
        .max(qqq.last_close_ts)
        .max(iwm.last_close_ts);
    anyhow::ensure!(
        max_end.saturating_sub(min_end) <= MAX_HISTORY_END_DATE_SKEW_SECS,
        "history sources end on materially different dates"
    );
    Ok(min_end)
}

fn log_returns(closes: &[f64]) -> Result<Vec<f64>> {
    anyhow::ensure!(
        closes.len() > REGRESSION_WINDOW,
        "not enough closes for returns"
    );
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
    let latest_close_s6 = scale_f64_to_s6(latest_close)?;
    let diff = (i128::from(latest_close_s6) - i128::from(spot_s6)).abs() as u128;
    let deviation_bps = diff.checked_mul(10_000).context("price sanity overflow")?
        / u128::try_from(latest_close_s6).context("negative latest close")?;
    anyhow::ensure!(
        deviation_bps <= REGRESSION_PRICE_SANITY_BPS as u128,
        "{symbol} latest close diverges from Pyth by {deviation_bps} bps"
    );
    Ok(())
}

fn solve_regression(spy: &[f64], qqq: &[f64], iwm: &[f64]) -> Result<RegressionSnapshot> {
    let n = spy.len().min(qqq.len()).min(iwm.len());
    anyhow::ensure!(
        n >= REGRESSION_WINDOW,
        "insufficient overlapping return samples for regression"
    );
    let spy = &spy[spy.len() - REGRESSION_WINDOW..];
    let qqq = &qqq[qqq.len() - REGRESSION_WINDOW..];
    let iwm = &iwm[iwm.len() - REGRESSION_WINDOW..];

    let mut x = DMatrix::<f64>::zeros(REGRESSION_WINDOW, 3);
    let mut y = DVector::<f64>::zeros(REGRESSION_WINDOW);
    for row in 0..REGRESSION_WINDOW {
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
    mean_y /= REGRESSION_WINDOW as f64;
    let mut sst = 0.0f64;
    for row in 0..REGRESSION_WINDOW {
        let resid = y[row] - fitted[row];
        sse += resid * resid;
        let centered = y[row] - mean_y;
        sst += centered * centered;
    }
    let dof = (REGRESSION_WINDOW as f64 - 3.0).max(1.0);
    let residual_std = (sse / dof).sqrt();
    let residual_vol_annualised = residual_std * (252.0f64).sqrt();
    let r_squared = if sst > 0.0 { 1.0 - (sse / sst) } else { 0.0 };

    Ok(RegressionSnapshot {
        alpha: beta[0],
        beta_spy: beta[1],
        beta_qqq: beta[2],
        r_squared,
        residual_vol_annualised,
        sample_count: REGRESSION_WINDOW,
    })
}

fn build_write_regression_args(
    regression: &RegressionSnapshot,
    window_end_ts: i64,
) -> Result<halcyon_kernel::WriteRegressionArgs> {
    Ok(halcyon_kernel::WriteRegressionArgs {
        beta_spy_s12: scale_f64_to_s12(regression.beta_spy)?,
        beta_qqq_s12: scale_f64_to_s12(regression.beta_qqq)?,
        alpha_s12: scale_f64_to_s12(regression.alpha)?,
        r_squared_s6: scale_f64_to_s6(regression.r_squared)?,
        residual_vol_s6: scale_f64_to_s6(regression.residual_vol_annualised)?,
        window_start_ts: window_end_ts.saturating_sub((REGRESSION_WINDOW as i64) * 86_400),
        window_end_ts,
        sample_count: u32::try_from(regression.sample_count).context("sample_count overflow")?,
    })
}

fn validate_write_regression_args(args: &halcyon_kernel::WriteRegressionArgs) -> Result<()> {
    anyhow::ensure!(
        (MIN_REGRESSION_BETA_S12..=MAX_REGRESSION_BETA_S12).contains(&args.beta_spy_s12),
        "beta_spy_s12 {} outside [{MIN_REGRESSION_BETA_S12}, {MAX_REGRESSION_BETA_S12}]",
        args.beta_spy_s12
    );
    anyhow::ensure!(
        (MIN_REGRESSION_BETA_S12..=MAX_REGRESSION_BETA_S12).contains(&args.beta_qqq_s12),
        "beta_qqq_s12 {} outside [{MIN_REGRESSION_BETA_S12}, {MAX_REGRESSION_BETA_S12}]",
        args.beta_qqq_s12
    );
    anyhow::ensure!(
        (-MAX_ABS_REGRESSION_ALPHA_S12..=MAX_ABS_REGRESSION_ALPHA_S12).contains(&args.alpha_s12),
        "alpha_s12 {} outside +/-{}",
        args.alpha_s12,
        MAX_ABS_REGRESSION_ALPHA_S12
    );
    anyhow::ensure!(
        (0..=MAX_REGRESSION_R_SQUARED_S6).contains(&args.r_squared_s6),
        "r_squared_s6 {} outside [0, {MAX_REGRESSION_R_SQUARED_S6}]",
        args.r_squared_s6
    );
    anyhow::ensure!(
        args.residual_vol_s6 >= 0,
        "residual_vol_s6 {} must be non-negative",
        args.residual_vol_s6
    );
    anyhow::ensure!(
        args.sample_count >= MIN_REGRESSION_SAMPLE_COUNT,
        "sample_count {} below minimum {}",
        args.sample_count,
        MIN_REGRESSION_SAMPLE_COUNT
    );
    anyhow::ensure!(
        args.window_end_ts > args.window_start_ts,
        "window_end_ts must be greater than window_start_ts"
    );
    Ok(())
}

fn scale_f64_to_s6(value: f64) -> Result<i64> {
    anyhow::ensure!(value.is_finite(), "value must be finite");
    Ok((value * 1_000_000.0).round() as i64)
}

fn scale_f64_to_s12(value: f64) -> Result<i128> {
    anyhow::ensure!(value.is_finite(), "value must be finite");
    Ok((value * 1_000_000_000_000.0).round() as i128)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
