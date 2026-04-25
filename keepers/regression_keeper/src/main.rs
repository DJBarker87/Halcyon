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
    hash::hashv,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    signer::Signer,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use tracing::{error, info, warn};

const WINDOW: usize = 252;
const MIN_REWRITE_GAP_SECS: i64 = 18 * 60 * 60;
const PRICE_SANITY_BPS: u64 = 100;
const MAX_PYTH_CLOCK_SKEW_SECS: i64 = 5;
const PYTH_BENCHMARKS_BASE_URL: &str = "https://benchmarks.pyth.network";
const PYTH_HISTORY_LOOKBACK_DAYS: i64 = 730;
const PYTH_MAX_HISTORY_RANGE_DAYS: i64 = 364;
const PYTH_CACHE_FRESH_GRACE_SECS: i64 = 3 * 86_400;
const USER_AGENT: &str = "Halcyon Regression Keeper";
const MEMO_PROGRAM_ID: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";

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
    #[serde(default = "default_spy_history_source")]
    spy_history_source: String,
    #[serde(default = "default_qqq_history_source")]
    qqq_history_source: String,
    #[serde(default = "default_iwm_history_source")]
    iwm_history_source: String,
    #[serde(default = "default_pyth_benchmarks_base_url")]
    pyth_benchmarks_base_url: String,
    #[serde(default = "default_history_cache_dir")]
    history_cache_dir: String,
    #[serde(default = "default_scan_interval_secs")]
    scan_interval_secs: u64,
    #[serde(default = "default_backoff_cap_secs")]
    backoff_cap_secs: u64,
    #[serde(default = "default_failure_budget")]
    failure_budget: u32,
}

fn default_spy_history_source() -> String {
    "pyth:SPY_USD".to_string()
}

fn default_qqq_history_source() -> String {
    "pyth:QQQ_USD".to_string()
}

fn default_iwm_history_source() -> String {
    "pyth:IWM_USD".to_string()
}

fn default_pyth_benchmarks_base_url() -> String {
    PYTH_BENCHMARKS_BASE_URL.to_string()
}

fn default_history_cache_dir() -> String {
    "/var/lib/halcyon/pyth-history".to_string()
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
        source_hash_hex: &str,
    ) -> Result<Signature> {
        let memo = Instruction {
            program_id: Pubkey::from_str(MEMO_PROGRAM_ID).expect("valid memo program id"),
            accounts: vec![],
            data: format!(
                "halcyon-regression-v1 source=pyth-benchmarks hash={source_hash_hex} window_end_ts={} sample_count={}",
                args.window_end_ts, args.sample_count
            )
            .into_bytes(),
        };
        let ix = kernel::write_regression_ix(&self.signer.pubkey(), &self.signer.pubkey(), args);
        send_instructions(&self.rpc, &self.signer, vec![memo, ix]).await
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

    let cache_dir = PathBuf::from(&cfg.history_cache_dir);
    let spy_series = load_close_series(
        &client.http,
        &cfg.spy_history_source,
        &cache_dir,
        &cfg.pyth_benchmarks_base_url,
    )
    .await?;
    let qqq_series = load_close_series(
        &client.http,
        &cfg.qqq_history_source,
        &cache_dir,
        &cfg.pyth_benchmarks_base_url,
    )
    .await?;
    let iwm_series = load_close_series(
        &client.http,
        &cfg.iwm_history_source,
        &cache_dir,
        &cfg.pyth_benchmarks_base_url,
    )
    .await?;

    let latest_spy_close = spy_series.latest_close()?;
    let latest_qqq_close = qqq_series.latest_close()?;
    let latest_iwm_close = iwm_series.latest_close()?;

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

    let source_hash_hex = regression_input_hash_hex(&spy_series, &qqq_series, &iwm_series)?;
    let spy_returns = log_returns(&spy_series.closes)?;
    let qqq_returns = log_returns(&qqq_series.closes)?;
    let iwm_returns = log_returns(&iwm_series.closes)?;
    let regression = solve_regression(&spy_returns, &qqq_returns, &iwm_returns)?;

    let sig = client
        .send_write_regression(
            halcyon_kernel::WriteRegressionArgs {
                beta_spy_s12: scale_to_s12(regression.beta_spy)?,
                beta_qqq_s12: scale_to_s12(regression.beta_qqq)?,
                alpha_s12: scale_to_s12(regression.alpha)?,
                r_squared_s6: scale_to_s6(regression.r_squared)?,
                residual_vol_s6: scale_to_s6(regression.residual_vol_annualised)?,
                window_start_ts: now.saturating_sub((WINDOW as i64) * 86_400),
                window_end_ts: now,
                sample_count: u32::try_from(regression.sample_count)
                    .context("sample_count overflow")?,
            },
            &source_hash_hex,
        )
        .await?;

    info!(
        target = "regression_keeper",
        beta_spy = regression.beta_spy,
        beta_qqq = regression.beta_qqq,
        alpha = regression.alpha,
        r_squared = regression.r_squared,
        residual_vol = regression.residual_vol_annualised,
        sample_count = regression.sample_count,
        source_hash = %source_hash_hex,
        spy_history_end_ts = spy_series.last_close_ts,
        qqq_history_end_ts = qqq_series.last_close_ts,
        iwm_history_end_ts = iwm_series.last_close_ts,
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

#[derive(Debug, Clone)]
struct HistoricalSeries {
    alias: String,
    closes: Vec<f64>,
    timestamps: Vec<i64>,
    last_close_ts: i64,
}

impl HistoricalSeries {
    fn latest_close(&self) -> Result<f64> {
        self.closes
            .last()
            .copied()
            .with_context(|| format!("missing latest {} close", self.alias))
    }
}

#[derive(Debug, Clone)]
struct PythHistorySource {
    alias: &'static str,
    symbol: &'static str,
}

async fn load_close_series(
    client: &reqwest::Client,
    source: &str,
    cache_dir: &Path,
    benchmarks_base_url: &str,
) -> Result<HistoricalSeries> {
    if let Some(identifier) = source.strip_prefix("pyth:") {
        let pyth_source = resolve_pyth_history_source(identifier)?;
        return fetch_pyth_history(client, pyth_source, cache_dir, benchmarks_base_url).await;
    }
    anyhow::ensure!(
        !source.starts_with("http://") && !source.starts_with("https://"),
        "HTTP history sources are disabled for regression; use pyth:SPY_USD/pyth:QQQ_USD/pyth:IWM_USD or a vetted local Pyth cache CSV"
    );
    let body = fs::read_to_string(source)
        .with_context(|| format!("reading vetted Pyth history cache from {source}"))?;
    parse_history_csv(source, &body)
}

fn resolve_pyth_history_source(identifier: &str) -> Result<PythHistorySource> {
    let normalized = identifier
        .trim()
        .trim_start_matches("0x")
        .replace(['-', '/'], "_")
        .to_ascii_uppercase();
    match normalized.as_str() {
        "SPY" | "SPY_USD" | "19E09BB805456ADA3979A7D1CBB4B6D63BABC3A0F8E8A9509F68AFA5C4C11CD5" => {
            Ok(PythHistorySource {
                alias: "SPY_USD",
                symbol: "Equity.US.SPY/USD",
            })
        }
        "QQQ" | "QQQ_USD" | "9695E2B96EA7B3859DA9ED25B7A46A920A776E2FDAE19A7BCFDF2B219230452D" => {
            Ok(PythHistorySource {
                alias: "QQQ_USD",
                symbol: "Equity.US.QQQ/USD",
            })
        }
        "IWM" | "IWM_USD" | "EFF690A187797AA225723345D4612ABEC0BF0CEC1AE62347C0E7B1905D730879" => {
            Ok(PythHistorySource {
                alias: "IWM_USD",
                symbol: "Equity.US.IWM/USD",
            })
        }
        other => anyhow::bail!("unsupported Pyth regression history source '{other}'"),
    }
}

async fn fetch_pyth_history(
    client: &reqwest::Client,
    source: PythHistorySource,
    cache_dir: &Path,
    benchmarks_base_url: &str,
) -> Result<HistoricalSeries> {
    let cache_path = cache_dir.join(format!("{}.csv", source.alias));
    if let Ok(body) = fs::read_to_string(&cache_path) {
        if let Ok(series) = parse_history_csv(source.alias, &body) {
            let today = utc_midnight_ts(unix_now());
            if today.saturating_sub(series.last_close_ts) <= PYTH_CACHE_FRESH_GRACE_SECS {
                return Ok(series);
            }
        }
    }

    let mut rows = Vec::new();
    let final_ts = utc_midnight_ts(unix_now());
    let mut from_ts = final_ts.saturating_sub(PYTH_HISTORY_LOOKBACK_DAYS * 86_400);
    while from_ts <= final_ts {
        let to_ts = (from_ts + PYTH_MAX_HISTORY_RANGE_DAYS * 86_400).min(final_ts);
        rows.extend(
            fetch_pyth_history_chunk(client, source.symbol, from_ts, to_ts, benchmarks_base_url)
                .await?,
        );
        from_ts = to_ts.saturating_add(86_400);
    }
    anyhow::ensure!(
        rows.len() > WINDOW,
        "not enough Pyth Benchmark rows for {}",
        source.alias
    );

    fs::create_dir_all(cache_dir)
        .with_context(|| format!("creating Pyth history cache dir {}", cache_dir.display()))?;
    let mut csv = String::from("date,open,high,low,close,volume\n");
    for (ts, close) in &rows {
        csv.push_str(&format!("{},,,,{:.10},\n", format_unix_date(*ts), close));
    }
    fs::write(&cache_path, csv)
        .with_context(|| format!("writing Pyth history cache {}", cache_path.display()))?;

    let body = fs::read_to_string(&cache_path)
        .with_context(|| format!("reading Pyth history cache {}", cache_path.display()))?;
    parse_history_csv(source.alias, &body)
}

async fn fetch_pyth_history_chunk(
    client: &reqwest::Client,
    symbol: &str,
    from_ts: i64,
    to_ts: i64,
    benchmarks_base_url: &str,
) -> Result<Vec<(i64, f64)>> {
    #[derive(Deserialize)]
    struct TradingViewHistory {
        s: String,
        #[serde(default)]
        t: Vec<i64>,
        #[serde(default)]
        c: Vec<Option<f64>>,
    }

    let payload: TradingViewHistory = client
        .get(format!(
            "{}/v1/shims/tradingview/history",
            benchmarks_base_url.trim_end_matches('/')
        ))
        .header("user-agent", USER_AGENT)
        .query(&[
            ("symbol", symbol.to_string()),
            ("resolution", "1D".to_string()),
            ("from", from_ts.to_string()),
            ("to", to_ts.to_string()),
        ])
        .send()
        .await
        .with_context(|| format!("fetching Pyth Benchmark history for {symbol}"))?
        .error_for_status()
        .with_context(|| format!("Pyth Benchmark history response status for {symbol}"))?
        .json()
        .await
        .with_context(|| format!("decoding Pyth Benchmark history for {symbol}"))?;
    anyhow::ensure!(
        payload.s == "ok",
        "Pyth Benchmark history returned status {} for {symbol}",
        payload.s
    );

    let mut rows = Vec::new();
    for (ts, close) in payload.t.into_iter().zip(payload.c.into_iter()) {
        let Some(close) = close else { continue };
        if close.is_finite() && close > 0.0 {
            rows.push((ts, close));
        }
    }
    Ok(rows)
}

fn parse_history_csv(alias: &str, raw: &str) -> Result<HistoricalSeries> {
    let mut closes = Vec::new();
    let mut timestamps = Vec::new();
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
            timestamps.push(parse_history_date_to_unix_ts(date)?);
        }
    }
    anyhow::ensure!(
        closes.len() > WINDOW,
        "not enough closes in {alias} historical series"
    );
    let last_close_ts = *timestamps.last().context("missing history end date")?;
    Ok(HistoricalSeries {
        alias: alias.to_string(),
        closes,
        timestamps,
        last_close_ts,
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
    Ok(date_to_unix_ts(year, month, day))
}

fn date_to_unix_ts(year: i32, month: u32, day: u32) -> i64 {
    let month = month as i64;
    let day = day as i64;
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * month_prime + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * 86_400
}

fn utc_midnight_ts(ts: i64) -> i64 {
    ts.div_euclid(86_400) * 86_400
}

fn format_unix_date(ts: i64) -> String {
    let days = ts.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month as u32, day as u32)
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

fn regression_input_hash_hex(
    spy: &HistoricalSeries,
    qqq: &HistoricalSeries,
    iwm: &HistoricalSeries,
) -> Result<String> {
    let mut payload = String::from("halcyon-regression-v1\nsource=pyth-benchmarks\n");
    for series in [spy, qqq, iwm] {
        anyhow::ensure!(
            series.closes.len() == series.timestamps.len(),
            "history timestamp/close length mismatch for {}",
            series.alias
        );
        let needed = WINDOW + 1;
        anyhow::ensure!(
            series.closes.len() >= needed,
            "not enough closes to hash regression input window for {}",
            series.alias
        );
        let start = series.closes.len() - needed;
        payload.push_str(&format!("asset={}\n", series.alias));
        for idx in start..series.closes.len() {
            payload.push_str(&format!(
                "{}, {:.10}\n",
                series.timestamps[idx], series.closes[idx]
            ));
        }
    }
    Ok(hex_string(&hashv(&[payload.as_bytes()]).to_bytes()))
}

fn hex_string(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
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
