use std::fs;
use std::path::PathBuf;

use halcyon_il_quote::insurance::european_nig::nig_european_il_premium;
use halcyon_il_quote::insurance::settlement::compute_settlement_from_prices;
use serde::{Deserialize, Serialize};

const SCALE_6_F64: f64 = 1_000_000.0;
const SCALE_12_F64: f64 = 1_000_000_000_000.0;
const SCALE_12_U64: u64 = 1_000_000_000_000;
const SCALE_12_U128: u128 = 1_000_000_000_000;
const USDC_DECIMALS: f64 = 1_000_000.0;
const DEFAULT_ALPHA: f64 = 3.1401;
const DEFAULT_BETA: f64 = 1.2139;
const DEFAULT_TENOR_DAYS: u32 = 30;
const DEFAULT_DEDUCTIBLE: f64 = 0.01;
const DEFAULT_CAP: f64 = 0.07;
const DEFAULT_LOAD: f64 = 1.10;
const DEFAULT_SIGMA_FLOOR: f64 = 0.40;
// Regime multipliers match the IL backtest (research/il_hedge_challenger.py +
// research_imports/folio_2026-04-10/scripts/il_regime_sigma_full_backtest.py).
// Pipeline: sigma = max(ewma45 * multiplier, 0.40); multiplier = 2.00 if fvol >= 0.60, else 1.30.
const SIGMA_MULTIPLIER_CALM: f64 = 1.30;
const SIGMA_MULTIPLIER_STRESS: f64 = 2.00;
const FVOL_STRESS_THRESHOLD: f64 = 0.60;

#[derive(Debug, Deserialize)]
struct BatchRequest {
    tasks: Vec<TaskRequest>,
}

#[derive(Debug, Deserialize)]
struct TaskRequest {
    task_id: String,
    insured_notional_usdc: f64,
    entry_sol_price_usd: f64,
    expiry_sol_price_usd: Option<f64>,
    sigma_ann: Option<f64>,
    ewma45_ann: Option<f64>,
    /// Forward volatility-of-volatility signal used to pick the regime
    /// multiplier. `fvol >= 0.60` → stress (×2.00); otherwise → calm (×1.30).
    /// If omitted, the regime defaults to calm — callers that do not provide
    /// `fvol` implicitly get the pre-stress-switch pipeline. Production callers
    /// should always supply fvol from the regime keeper.
    fvol: Option<f64>,
    alpha: Option<f64>,
    beta: Option<f64>,
    tenor_days: Option<u32>,
    deductible: Option<f64>,
    cap: Option<f64>,
    launch_load: Option<f64>,
}

#[derive(Debug, Serialize)]
struct BatchResponse {
    tasks: Vec<TaskResponse>,
}

#[derive(Debug, Serialize)]
struct TaskResponse {
    task_id: String,
    ok: bool,
    error: Option<String>,
    quote: Option<QuoteOutput>,
    settlement: Option<SettlementOutput>,
}

#[derive(Debug, Serialize)]
struct QuoteOutput {
    sigma_ann: f64,
    sigma_regime: &'static str,
    sigma_multiplier: f64,
    tenor_days: u32,
    deductible: f64,
    cap: f64,
    alpha: f64,
    beta: f64,
    fair_premium_fraction: f64,
    loaded_premium_fraction: f64,
    premium_usdc: f64,
}

#[derive(Debug, Serialize)]
struct SettlementOutput {
    terminal_il_fraction: f64,
    payout_fraction: f64,
    payout_usdc: f64,
    seller_edge_usdc: f64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let input_path = PathBuf::from(args.next().ok_or("missing input path")?);
    let output_path = PathBuf::from(args.next().ok_or("missing output path")?);

    let request: BatchRequest = serde_json::from_str(&fs::read_to_string(&input_path)?)?;
    let tasks = request.tasks.into_iter().map(run_task).collect::<Vec<_>>();
    fs::write(
        output_path,
        serde_json::to_string_pretty(&BatchResponse { tasks })?,
    )?;
    Ok(())
}

fn run_task(task: TaskRequest) -> TaskResponse {
    match run_task_inner(&task) {
        Ok((quote, settlement)) => TaskResponse {
            task_id: task.task_id.clone(),
            ok: true,
            error: None,
            quote: Some(quote),
            settlement,
        },
        Err(error) => TaskResponse {
            task_id: task.task_id.clone(),
            ok: false,
            error: Some(error.to_string()),
            quote: None,
            settlement: None,
        },
    }
}

fn run_task_inner(
    task: &TaskRequest,
) -> Result<(QuoteOutput, Option<SettlementOutput>), Box<dyn std::error::Error>> {
    if !(task.insured_notional_usdc.is_finite() && task.insured_notional_usdc > 0.0) {
        return Err("insured_notional_usdc must be positive".into());
    }
    if !(task.entry_sol_price_usd.is_finite() && task.entry_sol_price_usd > 0.0) {
        return Err("entry_sol_price_usd must be positive".into());
    }

    let tenor_days = task.tenor_days.unwrap_or(DEFAULT_TENOR_DAYS);
    let deductible = task.deductible.unwrap_or(DEFAULT_DEDUCTIBLE);
    let cap = task.cap.unwrap_or(DEFAULT_CAP);
    let alpha = task.alpha.unwrap_or(DEFAULT_ALPHA);
    let beta = task.beta.unwrap_or(DEFAULT_BETA);
    let launch_load = task.launch_load.unwrap_or(DEFAULT_LOAD);
    let (sigma_ann, sigma_regime, sigma_multiplier) = resolve_sigma(task);

    let fair_premium_fraction = nig_european_il_premium(
        scale_6_i64(sigma_ann),
        tenor_days,
        scale_6_i64(deductible),
        scale_6_i64(cap),
        scale_6_i64(alpha),
        scale_6_i64(beta),
    )
    .map_err(|err| std::io::Error::other(format!("{err:?}")))?
        as f64
        / SCALE_6_F64;
    let loaded_premium_fraction = fair_premium_fraction * launch_load;
    let premium_usdc = loaded_premium_fraction * task.insured_notional_usdc;

    let quote = QuoteOutput {
        sigma_ann,
        sigma_regime,
        sigma_multiplier,
        tenor_days,
        deductible,
        cap,
        alpha,
        beta,
        fair_premium_fraction,
        loaded_premium_fraction,
        premium_usdc,
    };

    let settlement = task
        .expiry_sol_price_usd
        .map(|expiry_price| settlement_output(task, expiry_price, deductible, cap, premium_usdc))
        .transpose()?;

    Ok((quote, settlement))
}

/// Returns `(sigma_ann, regime_label, multiplier_applied)` for the task.
///
/// - If the caller supplies an explicit `sigma_ann`, that value is used as-is
///   (regime label "explicit", multiplier 1.0).
/// - Otherwise, if `ewma45_ann` is supplied, the multiplier is chosen by the
///   fvol regime switch: `fvol >= 0.60` → stress (×2.00); else → calm (×1.30).
///   The 40% annualised floor is applied last.
/// - If neither `sigma_ann` nor `ewma45_ann` is supplied, the 40% floor is
///   returned directly (regime label "floor", multiplier 1.0).
///
/// This matches the IL backtest pipeline in
/// `research/il_hedge_challenger.py` and
/// `research_imports/folio_2026-04-10/scripts/il_regime_sigma_full_backtest.py`.
fn resolve_sigma(task: &TaskRequest) -> (f64, &'static str, f64) {
    if let Some(sigma_ann) = task.sigma_ann {
        return (sigma_ann, "explicit", 1.0);
    }
    if let Some(ewma45_ann) = task.ewma45_ann {
        let is_stress = task
            .fvol
            .map(|f| f.is_finite() && f >= FVOL_STRESS_THRESHOLD)
            .unwrap_or(false);
        let (multiplier, regime) = if is_stress {
            (SIGMA_MULTIPLIER_STRESS, "stress")
        } else {
            (SIGMA_MULTIPLIER_CALM, "calm")
        };
        return (
            (ewma45_ann * multiplier).max(DEFAULT_SIGMA_FLOOR),
            regime,
            multiplier,
        );
    }
    (DEFAULT_SIGMA_FLOOR, "floor", 1.0)
}

fn settlement_output(
    task: &TaskRequest,
    expiry_price: f64,
    deductible: f64,
    cap: f64,
    premium_usdc: f64,
) -> Result<SettlementOutput, Box<dyn std::error::Error>> {
    if !(expiry_price.is_finite() && expiry_price > 0.0) {
        return Err("expiry_sol_price_usd must be positive".into());
    }

    let (terminal_il, payout_raw) = compute_settlement_from_prices(
        SCALE_12_U64 / 2,
        scale_12_u128(expiry_price),
        SCALE_12_U128,
        scale_12_u128(task.entry_sol_price_usd),
        SCALE_12_U128,
        raw_usdc(task.insured_notional_usdc),
        scale_12_u64(deductible),
        scale_12_u64(cap),
    )
    .map_err(|err| std::io::Error::other(format!("{err:?}")))?;

    let payout_usdc = payout_raw as f64 / USDC_DECIMALS;
    Ok(SettlementOutput {
        terminal_il_fraction: terminal_il as f64 / SCALE_12_F64,
        payout_fraction: payout_usdc / task.insured_notional_usdc,
        payout_usdc,
        seller_edge_usdc: premium_usdc - payout_usdc,
    })
}

fn scale_6_i64(value: f64) -> i64 {
    (value * SCALE_6_F64).round() as i64
}

fn scale_12_u64(value: f64) -> u64 {
    (value * SCALE_12_F64).round() as u64
}

fn scale_12_u128(value: f64) -> u128 {
    (value * SCALE_12_F64).round() as u128
}

fn raw_usdc(value: f64) -> u64 {
    (value * USDC_DECIMALS).round() as u64
}
