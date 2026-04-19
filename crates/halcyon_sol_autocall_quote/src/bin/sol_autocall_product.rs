use std::fs;
use std::path::PathBuf;

use halcyon_sol_autocall_quote::autocall_hedged::{
    price_hedged_autocall, AutocallTerms, CouponQuoteMode, CouponVaultMode, HedgeFundingMode,
    HedgeMode, HedgePolicy, PathPoint, PricingModel,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct BatchRequest {
    tasks: Vec<TaskRequest>,
}

#[derive(Debug, Deserialize)]
struct TaskRequest {
    task_id: String,
    terms: TermsInput,
    model: Option<ModelInput>,
    policy: PolicyInput,
    path: Vec<PathPointInput>,
}

#[derive(Debug, Deserialize)]
struct TermsInput {
    notional: Option<f64>,
    entry_level: Option<f64>,
    maturity_days: usize,
    observation_interval_days: usize,
    autocall_barrier: f64,
    coupon_barrier: f64,
    knock_in_barrier: f64,
    coupon_quote_mode: Option<CouponQuoteModeInput>,
    issuer_margin_bps: f64,
    quote_share_of_fair_coupon: f64,
    note_id: Option<String>,
    engine_version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CouponQuoteModeInput {
    ShareOfFair,
    FixedPerObservation { value: f64 },
}

#[derive(Debug, Deserialize)]
struct ModelInput {
    sigma_ann: Option<f64>,
    alpha: Option<f64>,
    beta: Option<f64>,
    grid_points: Option<usize>,
    log_step: Option<f64>,
    cos_terms: Option<usize>,
    truncation_std: Option<f64>,
    kernel_std_width: Option<f64>,
    bridge_tail_factor: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct PolicyInput {
    hedge_mode: HedgeModeInput,
    initial_hedge_fraction: f64,
    delta_clip: f64,
    rebalance_band: f64,
    min_trade_notional_pct: f64,
    max_trade_notional_pct: Option<f64>,
    slippage_bps: f64,
    slippage_coeff: Option<f64>,
    liquidity_proxy_usdc: Option<f64>,
    slippage_stress_multiplier: Option<f64>,
    keeper_bounty_usdc: Option<f64>,
    cooldown_days: Option<usize>,
    max_rebalances_per_day: Option<usize>,
    force_observation_review: Option<bool>,
    allow_intraperiod_checks: Option<bool>,
    hedge_funding_mode: FundingModeInput,
    coupon_vault_mode: CouponVaultModeInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum HedgeModeInput {
    None,
    StaticFraction,
    DeltaObsOnly,
    DeltaDaily,
    DeltaObsPlusThreshold,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FundingModeInput {
    SeparateHedgeSleeve,
    UnderwritingVault,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CouponVaultModeInput {
    SeparateCouponVault,
    SharedUnderwriting,
}

#[derive(Debug, Deserialize)]
struct PathPointInput {
    day: usize,
    close: f64,
    low: f64,
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
    simulation: Option<SimulationOutput>,
}

#[derive(Debug, Serialize)]
struct QuoteOutput {
    fair_coupon_per_observation: f64,
    quoted_coupon_per_observation: f64,
    expected_payout: f64,
    expected_coupon_stream: f64,
    max_liability: f64,
    reserve_need: f64,
    expected_coupon_count: f64,
    expected_redemption: f64,
    fair_edge: f64,
    explicit_margin: f64,
    atm_delta: f64,
    runtime: RuntimeOutput,
}

#[derive(Debug, Serialize)]
struct RuntimeOutput {
    grid_points: usize,
    log_step: f64,
    day_steps: usize,
    observation_days: Vec<usize>,
    kernel_half_width: usize,
    kernel_mass: f64,
    convolution_ops: usize,
    bridge_ops: usize,
    delta_ops: usize,
}

#[derive(Debug, Serialize)]
struct SimulationOutput {
    buyer_total_return: f64,
    buyer_annualized_return: f64,
    profitable_note: bool,
    autocalled: bool,
    knock_in_triggered: bool,
    coupon_observations_paid: usize,
    coupon_paid_total: f64,
    redemption_paid: f64,
    retained_principal: f64,
    explicit_margin: f64,
    hedge_pnl: f64,
    slippage_cost: f64,
    turnover: f64,
    trade_count: usize,
    missed_trade_count: usize,
    execution_cost_total: f64,
    net_vault_pnl: f64,
    peak_committed_capital: f64,
    peak_hedge_capital_draw: f64,
    peak_coupon_capital_draw: f64,
    shortfall_flag: bool,
    insolvency_flag: bool,
    step_count: usize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let input_path = PathBuf::from(args.next().ok_or("missing input path")?);
    let output_path = PathBuf::from(args.next().ok_or("missing output path")?);

    let request: BatchRequest = serde_json::from_str(&fs::read_to_string(&input_path)?)?;
    let mut tasks = Vec::with_capacity(request.tasks.len());
    for task in request.tasks {
        tasks.push(run_task(task));
    }

    fs::write(
        output_path,
        serde_json::to_string_pretty(&BatchResponse { tasks })?,
    )?;
    Ok(())
}

fn run_task(task: TaskRequest) -> TaskResponse {
    match run_task_inner(&task) {
        Ok((quote, simulation)) => TaskResponse {
            task_id: task.task_id,
            ok: true,
            error: None,
            quote: Some(quote),
            simulation: Some(simulation),
        },
        Err(error) => TaskResponse {
            task_id: task.task_id,
            ok: false,
            error: Some(error.to_string()),
            quote: None,
            simulation: None,
        },
    }
}

fn run_task_inner(
    task: &TaskRequest,
) -> Result<(QuoteOutput, SimulationOutput), Box<dyn std::error::Error>> {
    let terms = map_terms(&task.terms);
    let model = map_model(task.model.as_ref());
    let policy = map_policy(&task.policy);
    let path = task
        .path
        .iter()
        .map(|point| PathPoint {
            day: point.day,
            close: point.close,
            low: point.low,
        })
        .collect::<Vec<_>>();

    let priced = price_hedged_autocall(&terms, &model)?;
    let replay = priced.simulate_path(&policy, &path)?;

    let quote = QuoteOutput {
        fair_coupon_per_observation: priced.pricing.fair_coupon_per_observation,
        quoted_coupon_per_observation: priced.pricing.quoted_coupon_per_observation,
        expected_payout: priced.pricing.expected_payout,
        expected_coupon_stream: priced.pricing.expected_coupon_stream,
        max_liability: priced.pricing.max_liability,
        reserve_need: priced.pricing.reserve_need,
        expected_coupon_count: priced.pricing.expected_coupon_count,
        expected_redemption: priced.pricing.expected_redemption,
        fair_edge: priced.pricing.fair_edge,
        explicit_margin: priced.pricing.explicit_margin,
        atm_delta: priced.pricing.atm_delta,
        runtime: RuntimeOutput {
            grid_points: priced.pricing.diagnostics.grid_points,
            log_step: priced.pricing.diagnostics.log_step,
            day_steps: priced.pricing.diagnostics.day_steps,
            observation_days: priced.pricing.diagnostics.observation_days.clone(),
            kernel_half_width: priced.pricing.diagnostics.kernel_half_width,
            kernel_mass: priced.pricing.diagnostics.kernel_mass,
            convolution_ops: priced.pricing.diagnostics.convolution_ops,
            bridge_ops: priced.pricing.diagnostics.bridge_ops,
            delta_ops: priced.pricing.diagnostics.delta_ops,
        },
    };

    let simulation = SimulationOutput {
        buyer_total_return: replay.buyer_total_return,
        buyer_annualized_return: replay.buyer_annualized_return,
        profitable_note: replay.profitable_note,
        autocalled: replay.autocalled,
        knock_in_triggered: replay.knock_in_triggered,
        coupon_observations_paid: replay.coupon_observations_paid,
        coupon_paid_total: replay.coupon_paid_total,
        redemption_paid: replay.redemption_paid,
        retained_principal: replay.retained_principal,
        explicit_margin: replay.explicit_margin,
        hedge_pnl: replay.hedge_pnl,
        slippage_cost: replay.slippage_cost,
        turnover: replay.turnover,
        trade_count: replay.trade_count,
        missed_trade_count: replay.missed_trade_count,
        execution_cost_total: replay.execution_cost_total,
        net_vault_pnl: replay.net_vault_pnl,
        peak_committed_capital: replay.accounting.peak_committed_capital,
        peak_hedge_capital_draw: replay.accounting.peak_hedge_capital_draw,
        peak_coupon_capital_draw: replay.accounting.peak_coupon_capital_draw,
        shortfall_flag: replay.accounting.shortfall_flag,
        insolvency_flag: replay.accounting.insolvency_flag,
        step_count: replay.steps.len(),
    };

    Ok((quote, simulation))
}

fn map_terms(input: &TermsInput) -> AutocallTerms {
    AutocallTerms {
        notional: input.notional.unwrap_or(1.0),
        entry_level: input.entry_level.unwrap_or(1.0),
        maturity_days: input.maturity_days,
        observation_interval_days: input.observation_interval_days,
        autocall_barrier: input.autocall_barrier,
        coupon_barrier: input.coupon_barrier,
        knock_in_barrier: input.knock_in_barrier,
        coupon_quote_mode: match input.coupon_quote_mode.as_ref() {
            Some(CouponQuoteModeInput::FixedPerObservation { value }) => {
                CouponQuoteMode::FixedPerObservation(*value)
            }
            _ => CouponQuoteMode::ShareOfFair,
        },
        issuer_margin_bps: input.issuer_margin_bps,
        quote_share_of_fair_coupon: input.quote_share_of_fair_coupon,
        note_id: input
            .note_id
            .clone()
            .unwrap_or_else(|| "sol_autocall_product".to_string()),
        engine_version: input.engine_version.clone().unwrap_or_default(),
        no_autocall_first_n_obs: 1,
    }
}

fn map_model(input: Option<&ModelInput>) -> PricingModel {
    let default = PricingModel::default();
    if let Some(input) = input {
        PricingModel {
            sigma_ann: input.sigma_ann.unwrap_or(default.sigma_ann),
            alpha: input.alpha.unwrap_or(default.alpha),
            beta: input.beta.unwrap_or(default.beta),
            grid_points: input.grid_points.unwrap_or(default.grid_points),
            log_step: input.log_step.unwrap_or(default.log_step),
            cos_terms: input.cos_terms.unwrap_or(default.cos_terms),
            truncation_std: input.truncation_std.unwrap_or(default.truncation_std),
            kernel_std_width: input.kernel_std_width.unwrap_or(default.kernel_std_width),
            bridge_tail_factor: input
                .bridge_tail_factor
                .unwrap_or(default.bridge_tail_factor),
        }
    } else {
        default
    }
}

fn map_policy(input: &PolicyInput) -> HedgePolicy {
    HedgePolicy {
        hedge_mode: match input.hedge_mode {
            HedgeModeInput::None => HedgeMode::None,
            HedgeModeInput::StaticFraction => HedgeMode::StaticFraction,
            HedgeModeInput::DeltaObsOnly => HedgeMode::DeltaObservationOnly,
            HedgeModeInput::DeltaDaily => HedgeMode::DeltaDaily,
            HedgeModeInput::DeltaObsPlusThreshold => HedgeMode::DeltaObservationPlusThreshold,
        },
        initial_hedge_fraction: input.initial_hedge_fraction,
        initial_hedge_from_model_delta: false,
        delta_clip: input.delta_clip,
        rebalance_band: input.rebalance_band,
        min_trade_notional_pct: input.min_trade_notional_pct,
        max_trade_notional_pct: input.max_trade_notional_pct.unwrap_or(1.0),
        slippage_bps: input.slippage_bps,
        slippage_coeff: input.slippage_coeff.unwrap_or(25.0),
        liquidity_proxy_usdc: input.liquidity_proxy_usdc.unwrap_or(250_000.0),
        slippage_stress_multiplier: input.slippage_stress_multiplier.unwrap_or(1.0),
        keeper_bounty_usdc: input.keeper_bounty_usdc.unwrap_or(0.10),
        cooldown_days: input.cooldown_days.unwrap_or(0),
        max_rebalances_per_day: input.max_rebalances_per_day.unwrap_or(8),
        force_observation_review: input.force_observation_review.unwrap_or(true),
        allow_intraperiod_checks: input.allow_intraperiod_checks.unwrap_or(true),
        downside_soft_threshold: 0.90,
        downside_deep_threshold: 0.80,
        downside_soft_cap: 0.50,
        downside_deep_cap: 0.30,
        hedge_funding_mode: match input.hedge_funding_mode {
            FundingModeInput::SeparateHedgeSleeve => HedgeFundingMode::SeparateHedgeSleeve,
            FundingModeInput::UnderwritingVault => HedgeFundingMode::UnderwritingVault,
        },
        coupon_vault_mode: match input.coupon_vault_mode {
            CouponVaultModeInput::SeparateCouponVault => CouponVaultMode::SeparateCouponVault,
            CouponVaultModeInput::SharedUnderwriting => CouponVaultMode::SharedUnderwriting,
        },
        ..HedgePolicy::default()
    }
}
