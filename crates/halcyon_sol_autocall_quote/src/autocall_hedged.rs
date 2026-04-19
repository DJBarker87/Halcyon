use std::collections::HashSet;
use std::f64::consts::PI;

use crate::autocall_v2_parity::price_autocall_v2_parity;
use crate::capital_stack::{CapitalArchitecture, CapitalStack};
use crate::hedge_controller::{
    evaluate_hedge_action, HedgeBlockReason, HedgeControllerConfig, HedgeControllerInput,
    HedgeControllerState, HedgeTargetPolicy, HedgeTriggerReason,
};
use crate::sol_swap_cost::SolSwapCostConfig;

pub const SOL_AUTOCALL_HEDGED_ENGINE_VERSION: &str = "sol_autocall_hedged_v2_live_quote_adapter";
pub const DEFAULT_SOL_NIG_ALPHA: f64 = 13.04;
pub const DEFAULT_SOL_NIG_BETA: f64 = 1.52;

#[derive(Debug, Clone, PartialEq)]
pub enum HedgedAutocallError {
    InvalidTerms(&'static str),
    InvalidModel(&'static str),
    InvalidPath(&'static str),
    Numerical(&'static str),
}

impl std::fmt::Display for HedgedAutocallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTerms(msg) => write!(f, "invalid terms: {msg}"),
            Self::InvalidModel(msg) => write!(f, "invalid model: {msg}"),
            Self::InvalidPath(msg) => write!(f, "invalid path: {msg}"),
            Self::Numerical(msg) => write!(f, "numerical error: {msg}"),
        }
    }
}

impl std::error::Error for HedgedAutocallError {}

#[derive(Debug, Clone, PartialEq)]
pub enum CouponQuoteMode {
    ShareOfFair,
    FixedPerObservation(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutocallTerms {
    pub notional: f64,
    pub entry_level: f64,
    pub maturity_days: usize,
    pub observation_interval_days: usize,
    pub autocall_barrier: f64,
    pub coupon_barrier: f64,
    pub knock_in_barrier: f64,
    pub coupon_quote_mode: CouponQuoteMode,
    pub issuer_margin_bps: f64,
    pub quote_share_of_fair_coupon: f64,
    pub note_id: String,
    pub engine_version: String,
    /// Number of initial observations where autocall is suppressed.
    /// Coupons and knock-in checks still apply. Default 1 (skip day-2 autocall).
    pub no_autocall_first_n_obs: usize,
}

impl AutocallTerms {
    pub fn current_v1(entry_level: f64) -> Self {
        Self {
            notional: 1.0,
            entry_level,
            maturity_days: 16,
            observation_interval_days: 2,
            autocall_barrier: 1.025,
            coupon_barrier: 1.0,
            knock_in_barrier: 0.70,
            coupon_quote_mode: CouponQuoteMode::ShareOfFair,
            issuer_margin_bps: 50.0,
            quote_share_of_fair_coupon: 0.75,
            note_id: "CURRENT_V1".to_string(),
            engine_version: SOL_AUTOCALL_HEDGED_ENGINE_VERSION.to_string(),
            no_autocall_first_n_obs: 1,
        }
    }

    fn validate(&self) -> Result<(), HedgedAutocallError> {
        if !(self.notional.is_finite() && self.notional > 0.0) {
            return Err(HedgedAutocallError::InvalidTerms(
                "notional must be positive",
            ));
        }
        if !(self.entry_level.is_finite() && self.entry_level > 0.0) {
            return Err(HedgedAutocallError::InvalidTerms(
                "entry_level must be positive",
            ));
        }
        if self.maturity_days == 0 {
            return Err(HedgedAutocallError::InvalidTerms(
                "maturity_days must be positive",
            ));
        }
        if self.observation_interval_days == 0 {
            return Err(HedgedAutocallError::InvalidTerms(
                "observation_interval_days must be positive",
            ));
        }
        if !(self.knock_in_barrier.is_finite()
            && self.coupon_barrier.is_finite()
            && self.autocall_barrier.is_finite())
        {
            return Err(HedgedAutocallError::InvalidTerms("barriers must be finite"));
        }
        if self.knock_in_barrier <= 0.0 {
            return Err(HedgedAutocallError::InvalidTerms(
                "knock_in_barrier must be positive",
            ));
        }
        if self.coupon_barrier <= self.knock_in_barrier {
            return Err(HedgedAutocallError::InvalidTerms(
                "coupon_barrier must sit above knock_in_barrier",
            ));
        }
        if self.autocall_barrier < self.coupon_barrier {
            return Err(HedgedAutocallError::InvalidTerms(
                "autocall_barrier must be at or above coupon_barrier",
            ));
        }
        if !(0.0..=1.0).contains(&self.quote_share_of_fair_coupon) {
            return Err(HedgedAutocallError::InvalidTerms(
                "quote_share_of_fair_coupon must be in [0,1]",
            ));
        }
        if !self.issuer_margin_bps.is_finite() {
            return Err(HedgedAutocallError::InvalidTerms(
                "issuer_margin_bps must be finite",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PricingModel {
    pub sigma_ann: f64,
    pub alpha: f64,
    pub beta: f64,
    pub grid_points: usize,
    pub log_step: f64,
    pub cos_terms: usize,
    pub truncation_std: f64,
    pub kernel_std_width: f64,
    pub bridge_tail_factor: f64,
}

impl Default for PricingModel {
    fn default() -> Self {
        Self {
            sigma_ann: 1.17,
            alpha: DEFAULT_SOL_NIG_ALPHA,
            beta: DEFAULT_SOL_NIG_BETA,
            grid_points: 161,
            log_step: 0.02,
            cos_terms: 48,
            truncation_std: 8.0,
            kernel_std_width: 6.0,
            bridge_tail_factor: 1.3,
        }
    }
}

impl PricingModel {
    fn validate(&self) -> Result<(), HedgedAutocallError> {
        if !(self.sigma_ann.is_finite() && self.sigma_ann > 0.0) {
            return Err(HedgedAutocallError::InvalidModel(
                "sigma_ann must be positive",
            ));
        }
        if !(self.alpha.is_finite() && self.alpha > 0.0 && self.beta.is_finite()) {
            return Err(HedgedAutocallError::InvalidModel(
                "alpha and beta must be finite",
            ));
        }
        if self.alpha <= self.beta.abs() || self.alpha <= (self.beta + 1.0).abs() {
            return Err(HedgedAutocallError::InvalidModel(
                "alpha must exceed |beta| and |beta + 1|",
            ));
        }
        if self.grid_points < 33 || self.grid_points % 2 == 0 {
            return Err(HedgedAutocallError::InvalidModel(
                "grid_points must be odd and at least 33",
            ));
        }
        if !(self.log_step.is_finite() && self.log_step > 0.0) {
            return Err(HedgedAutocallError::InvalidModel(
                "log_step must be positive",
            ));
        }
        if self.cos_terms < 8 {
            return Err(HedgedAutocallError::InvalidModel(
                "cos_terms must be at least 8",
            ));
        }
        if !(self.truncation_std.is_finite() && self.truncation_std > 0.0) {
            return Err(HedgedAutocallError::InvalidModel(
                "truncation_std must be positive",
            ));
        }
        if !(self.kernel_std_width.is_finite() && self.kernel_std_width > 0.0) {
            return Err(HedgedAutocallError::InvalidModel(
                "kernel_std_width must be positive",
            ));
        }
        if !(self.bridge_tail_factor.is_finite() && self.bridge_tail_factor > 0.0) {
            return Err(HedgedAutocallError::InvalidModel(
                "bridge_tail_factor must be positive",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibleState {
    Autocalled,
    CouponZone,
    NoCouponNoKnockInZone,
    NoCouponKnockInLatchedZone,
    KnockInTriggeredZone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HedgeMode {
    None,
    StaticFraction,
    DeltaObservationOnly,
    DeltaDaily,
    DeltaObservationPlusThreshold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HedgeFundingMode {
    SeparateHedgeSleeve,
    UnderwritingVault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CouponVaultMode {
    SeparateCouponVault,
    SharedUnderwriting,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HedgePolicy {
    pub hedge_mode: HedgeMode,
    pub initial_hedge_fraction: f64,
    pub initial_hedge_from_model_delta: bool,
    pub delta_clip: f64,
    pub rebalance_band: f64,
    pub min_trade_notional_pct: f64,
    pub max_trade_notional_pct: f64,
    pub slippage_bps: f64,
    pub slippage_coeff: f64,
    pub liquidity_proxy_usdc: f64,
    pub slippage_stress_multiplier: f64,
    pub keeper_bounty_usdc: f64,
    pub cooldown_days: usize,
    pub max_rebalances_per_day: usize,
    pub force_observation_review: bool,
    pub allow_intraperiod_checks: bool,
    pub target_policy: HedgeTargetPolicy,
    pub coupon_zone_delta_cap: f64,
    pub sub_coupon_delta_cap: f64,
    pub post_ki_delta_cap: f64,
    pub ki_taper_floor: f64,
    pub recovery_reentry_ratio: f64,
    pub recovery_reentry_delta_cap: f64,
    pub cost_aware_expected_move: f64,
    pub cost_aware_threshold_multiple: f64,
    pub call_zone_buffer: f64,
    pub downside_soft_threshold: f64,
    pub downside_deep_threshold: f64,
    pub downside_soft_cap: f64,
    pub downside_deep_cap: f64,
    pub coupon_zone_floor_lambda: f64,
    pub near_ki_lambda: f64,
    pub coupon_zone_band_multiplier: f64,
    pub observation_hysteresis_days: usize,
    pub rebound_unwind_half_life_days: f64,
    pub hedge_funding_mode: HedgeFundingMode,
    pub coupon_vault_mode: CouponVaultMode,
}

impl Default for HedgePolicy {
    fn default() -> Self {
        Self {
            hedge_mode: HedgeMode::None,
            initial_hedge_fraction: 0.0,
            initial_hedge_from_model_delta: false,
            delta_clip: 1.0,
            rebalance_band: 0.10,
            min_trade_notional_pct: 0.0,
            max_trade_notional_pct: 1.0,
            slippage_bps: 10.0,
            slippage_coeff: 25.0,
            liquidity_proxy_usdc: 250_000.0,
            slippage_stress_multiplier: 1.0,
            keeper_bounty_usdc: 0.10,
            cooldown_days: 0,
            max_rebalances_per_day: 8,
            force_observation_review: true,
            allow_intraperiod_checks: true,
            target_policy: HedgeTargetPolicy::RawDelta,
            coupon_zone_delta_cap: 1.0,
            sub_coupon_delta_cap: 1.0,
            post_ki_delta_cap: 0.0,
            ki_taper_floor: 0.0,
            recovery_reentry_ratio: 0.95,
            recovery_reentry_delta_cap: 0.35,
            cost_aware_expected_move: 0.02,
            cost_aware_threshold_multiple: 1.0,
            call_zone_buffer: 0.05,
            downside_soft_threshold: 0.90,
            downside_deep_threshold: 0.80,
            downside_soft_cap: 0.50,
            downside_deep_cap: 0.30,
            coupon_zone_floor_lambda: 1.0,
            near_ki_lambda: 1.0,
            coupon_zone_band_multiplier: 1.0,
            observation_hysteresis_days: 0,
            rebound_unwind_half_life_days: 0.0,
            hedge_funding_mode: HedgeFundingMode::SeparateHedgeSleeve,
            coupon_vault_mode: CouponVaultMode::SeparateCouponVault,
        }
    }
}

impl HedgePolicy {
    fn validate(&self) -> Result<(), HedgedAutocallError> {
        if !(0.0..=1.0).contains(&self.initial_hedge_fraction) {
            return Err(HedgedAutocallError::InvalidTerms(
                "initial_hedge_fraction must be in [0,1]",
            ));
        }
        if !(0.0..=1.0).contains(&self.delta_clip) || self.delta_clip == 0.0 {
            return Err(HedgedAutocallError::InvalidTerms(
                "delta_clip must be in (0,1]",
            ));
        }
        if !(0.0..=1.0).contains(&self.rebalance_band) {
            return Err(HedgedAutocallError::InvalidTerms(
                "rebalance_band must be in [0,1]",
            ));
        }
        if !(0.0..=1.0).contains(&self.min_trade_notional_pct) {
            return Err(HedgedAutocallError::InvalidTerms(
                "min_trade_notional_pct must be in [0,1]",
            ));
        }
        if !(0.0..=1.0).contains(&self.max_trade_notional_pct) || self.max_trade_notional_pct == 0.0
        {
            return Err(HedgedAutocallError::InvalidTerms(
                "max_trade_notional_pct must be in (0,1]",
            ));
        }
        if self.max_trade_notional_pct < self.min_trade_notional_pct {
            return Err(HedgedAutocallError::InvalidTerms(
                "max_trade_notional_pct must be >= min_trade_notional_pct",
            ));
        }
        if !(self.slippage_bps.is_finite() && self.slippage_bps >= 0.0) {
            return Err(HedgedAutocallError::InvalidTerms(
                "slippage_bps must be non-negative",
            ));
        }
        if !(self.slippage_coeff.is_finite() && self.slippage_coeff >= 0.0) {
            return Err(HedgedAutocallError::InvalidTerms(
                "slippage_coeff must be non-negative",
            ));
        }
        if !(self.liquidity_proxy_usdc.is_finite() && self.liquidity_proxy_usdc > 0.0) {
            return Err(HedgedAutocallError::InvalidTerms(
                "liquidity_proxy_usdc must be positive",
            ));
        }
        if !(self.slippage_stress_multiplier.is_finite() && self.slippage_stress_multiplier > 0.0) {
            return Err(HedgedAutocallError::InvalidTerms(
                "slippage_stress_multiplier must be positive",
            ));
        }
        if !(self.keeper_bounty_usdc.is_finite() && self.keeper_bounty_usdc >= 0.0) {
            return Err(HedgedAutocallError::InvalidTerms(
                "keeper_bounty_usdc must be non-negative",
            ));
        }
        if self.max_rebalances_per_day == 0 {
            return Err(HedgedAutocallError::InvalidTerms(
                "max_rebalances_per_day must be positive",
            ));
        }
        if !(0.0..=1.0).contains(&self.coupon_zone_delta_cap) {
            return Err(HedgedAutocallError::InvalidTerms(
                "coupon_zone_delta_cap must be in [0,1]",
            ));
        }
        if !(0.0..=1.0).contains(&self.sub_coupon_delta_cap) {
            return Err(HedgedAutocallError::InvalidTerms(
                "sub_coupon_delta_cap must be in [0,1]",
            ));
        }
        if !(0.0..=1.0).contains(&self.post_ki_delta_cap) {
            return Err(HedgedAutocallError::InvalidTerms(
                "post_ki_delta_cap must be in [0,1]",
            ));
        }
        if !(0.0..=1.0).contains(&self.ki_taper_floor) {
            return Err(HedgedAutocallError::InvalidTerms(
                "ki_taper_floor must be in [0,1]",
            ));
        }
        if !(self.recovery_reentry_ratio.is_finite() && self.recovery_reentry_ratio > 0.0) {
            return Err(HedgedAutocallError::InvalidTerms(
                "recovery_reentry_ratio must be positive",
            ));
        }
        if !(0.0..=1.0).contains(&self.recovery_reentry_delta_cap) {
            return Err(HedgedAutocallError::InvalidTerms(
                "recovery_reentry_delta_cap must be in [0,1]",
            ));
        }
        if !(0.0..=1.0).contains(&self.cost_aware_expected_move) {
            return Err(HedgedAutocallError::InvalidTerms(
                "cost_aware_expected_move must be in [0,1]",
            ));
        }
        if !(self.cost_aware_threshold_multiple.is_finite()
            && self.cost_aware_threshold_multiple >= 0.0)
        {
            return Err(HedgedAutocallError::InvalidTerms(
                "cost_aware_threshold_multiple must be non-negative",
            ));
        }
        if !(0.0..=1.0).contains(&self.call_zone_buffer) {
            return Err(HedgedAutocallError::InvalidTerms(
                "call_zone_buffer must be in [0,1]",
            ));
        }
        if !(0.0..=1.0).contains(&self.coupon_zone_floor_lambda) {
            return Err(HedgedAutocallError::InvalidTerms(
                "coupon_zone_floor_lambda must be in [0,1]",
            ));
        }
        if !(0.0..=1.0).contains(&self.near_ki_lambda) {
            return Err(HedgedAutocallError::InvalidTerms(
                "near_ki_lambda must be in [0,1]",
            ));
        }
        if !(self.coupon_zone_band_multiplier.is_finite()
            && self.coupon_zone_band_multiplier >= 1.0)
        {
            return Err(HedgedAutocallError::InvalidTerms(
                "coupon_zone_band_multiplier must be at least 1.0",
            ));
        }
        if !(self.rebound_unwind_half_life_days.is_finite()
            && self.rebound_unwind_half_life_days >= 0.0)
        {
            return Err(HedgedAutocallError::InvalidTerms(
                "rebound_unwind_half_life_days must be non-negative",
            ));
        }
        Ok(())
    }

    pub fn capital_architecture(&self) -> CapitalArchitecture {
        match (self.hedge_funding_mode, self.coupon_vault_mode) {
            (HedgeFundingMode::UnderwritingVault, _) => {
                CapitalArchitecture::UnderwritingFundedHedge
            }
            (HedgeFundingMode::SeparateHedgeSleeve, CouponVaultMode::SharedUnderwriting) => {
                CapitalArchitecture::SharedSleeves
            }
            (HedgeFundingMode::SeparateHedgeSleeve, CouponVaultMode::SeparateCouponVault) => {
                CapitalArchitecture::SeparateSleeves
            }
        }
    }

    fn controller_config(&self) -> HedgeControllerConfig {
        HedgeControllerConfig {
            hedge_mode: self.hedge_mode,
            initial_hedge_delta: self.initial_hedge_fraction,
            initial_hedge_from_model_delta: self.initial_hedge_from_model_delta,
            delta_clip: self.delta_clip,
            hedge_band: self.rebalance_band,
            cooldown_days: self.cooldown_days,
            min_trade_delta: self.min_trade_notional_pct,
            max_trade_delta: self.max_trade_notional_pct,
            max_rebalances_per_day: self.max_rebalances_per_day,
            force_observation_review: self.force_observation_review,
            allow_intraperiod_checks: self.allow_intraperiod_checks,
            swap_cost: SolSwapCostConfig {
                base_fee_bps: self.slippage_bps,
                slippage_coeff: self.slippage_coeff,
                liquidity_proxy: self.liquidity_proxy_usdc,
                stress_multiplier: self.slippage_stress_multiplier,
                keeper_bounty_usdc: self.keeper_bounty_usdc,
            },
            target_policy: self.target_policy,
            coupon_zone_delta_cap: self.coupon_zone_delta_cap,
            sub_coupon_delta_cap: self.sub_coupon_delta_cap,
            post_ki_delta_cap: self.post_ki_delta_cap,
            ki_taper_floor: self.ki_taper_floor,
            recovery_reentry_ratio: self.recovery_reentry_ratio,
            recovery_reentry_delta_cap: self.recovery_reentry_delta_cap,
            cost_aware_expected_move: self.cost_aware_expected_move,
            cost_aware_threshold_multiple: self.cost_aware_threshold_multiple,
            call_zone_buffer: self.call_zone_buffer,
            downside_soft_threshold: self.downside_soft_threshold,
            downside_deep_threshold: self.downside_deep_threshold,
            downside_soft_cap: self.downside_soft_cap,
            downside_deep_cap: self.downside_deep_cap,
            coupon_zone_floor_lambda: self.coupon_zone_floor_lambda,
            near_ki_lambda: self.near_ki_lambda,
            coupon_zone_band_multiplier: self.coupon_zone_band_multiplier,
            observation_hysteresis_days: self.observation_hysteresis_days,
            rebound_unwind_half_life_days: self.rebound_unwind_half_life_days,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeDiagnostics {
    pub grid_points: usize,
    pub log_step: f64,
    pub day_steps: usize,
    pub observation_days: Vec<usize>,
    pub kernel_half_width: usize,
    pub kernel_mass: f64,
    pub convolution_ops: usize,
    pub bridge_ops: usize,
    pub delta_ops: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PricingOutputs {
    pub fair_coupon_per_observation: f64,
    pub quoted_coupon_per_observation: f64,
    pub expected_payout: f64,
    pub expected_coupon_stream: f64,
    pub expected_liability_profile: Vec<f64>,
    pub max_liability: f64,
    pub reserve_need: f64,
    pub expected_coupon_count: f64,
    pub expected_redemption: f64,
    pub fair_edge: f64,
    pub explicit_margin: f64,
    pub atm_delta: f64,
    pub diagnostics: RuntimeDiagnostics,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HedgeDecision {
    pub target_hedge_ratio: f64,
    pub raw_delta: f64,
    pub clipped_delta: f64,
    pub policy_target_delta: f64,
    pub rebalance_action: f64,
    pub turnover: f64,
    pub fee_cost: f64,
    pub slippage_cost: f64,
    pub keeper_cost: f64,
    pub execution_cost_total: f64,
    pub trade_notional_usdc: f64,
    pub visible_state: VisibleState,
    pub tracking_error: f64,
    pub trade_count: usize,
    pub missed_trade: bool,
    pub trigger_reason: &'static str,
    pub block_reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PnlComponents {
    pub hedge_gross_pnl: f64,
    pub swap_fee_cost: f64,
    pub slippage_cost: f64,
    pub keeper_cost: f64,
    pub coupon_funding_cost: f64,
    pub reserve_carry: f64,
    pub knock_in_residual: f64,
    pub explicit_margin: f64,
    pub net_vault_pnl: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccountingState {
    pub escrowed_user_principal: f64,
    pub hedge_inventory_sol: f64,
    pub hedge_cash_usdc: f64,
    pub coupon_cash_usdc: f64,
    pub coupon_outflows: f64,
    pub underwriting_reserve: f64,
    pub accrued_unpaid_coupons: f64,
    pub issuer_margin_usdc: f64,
    pub hedge_pnl: f64,
    pub net_vault_pnl: f64,
    pub peak_hedge_capital_draw: f64,
    pub peak_coupon_capital_draw: f64,
    pub peak_committed_capital: f64,
    pub reserve_occupancy: f64,
    pub shortfall_flag: bool,
    pub insolvency_flag: bool,
    pub execution_fee_total: f64,
    pub execution_slippage_total: f64,
    pub keeper_cost_total: f64,
    pub hedge_gross_pnl: f64,
    pub pnl_components: PnlComponents,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathPoint {
    pub day: usize,
    pub close: f64,
    pub low: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SimulationStep {
    pub day: usize,
    pub close_ratio: f64,
    pub low_ratio: f64,
    pub observation_day: bool,
    pub visible_state: VisibleState,
    pub hedge_decision: HedgeDecision,
    pub coupon_paid: f64,
    pub autocalled: bool,
    pub hedge_inventory_sol: f64,
    pub hedge_cash_usdc: f64,
    pub coupon_outflows_cumulative: f64,
    pub reserve_occupancy: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathSimulationResult {
    pub buyer_total_return: f64,
    pub buyer_annualized_return: f64,
    pub profitable_note: bool,
    pub autocalled: bool,
    pub knock_in_triggered: bool,
    pub coupon_observations_paid: usize,
    pub coupon_paid_total: f64,
    pub redemption_paid: f64,
    pub retained_principal: f64,
    pub explicit_margin: f64,
    pub hedge_gross_pnl: f64,
    pub hedge_pnl: f64,
    pub fee_cost_total: f64,
    pub keeper_cost_total: f64,
    pub slippage_cost: f64,
    pub turnover: f64,
    pub trade_count: usize,
    pub missed_trade_count: usize,
    pub execution_cost_total: f64,
    pub net_vault_pnl: f64,
    pub pnl_components: PnlComponents,
    pub accounting: AccountingState,
    pub steps: Vec<SimulationStep>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DailySurface {
    pub day: usize,
    pub spot_ratios: Vec<f64>,
    pub untouched_values: Vec<f64>,
    pub touched_values: Vec<f64>,
    pub untouched_deltas: Vec<f64>,
    pub touched_deltas: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PricedAutocall {
    pub terms: AutocallTerms,
    pub model: PricingModel,
    pub pricing: PricingOutputs,
    pub observation_days: Vec<usize>,
    pub surfaces: Vec<DailySurface>,
}

#[derive(Debug, Clone, Copy)]
struct Complex64 {
    re: f64,
    im: f64,
}

impl Complex64 {
    fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
}

#[derive(Debug, Clone)]
struct NigStepModel {
    alpha: f64,
    beta: f64,
    gamma: f64,
    drift: f64,
    dt: f64,
    variance: f64,
    mean: f64,
}

impl NigStepModel {
    fn from_sigma(
        sigma_ann: f64,
        alpha: f64,
        beta: f64,
        step_days: f64,
    ) -> Result<Self, HedgedAutocallError> {
        if step_days <= 0.0 {
            return Err(HedgedAutocallError::InvalidModel(
                "step_days must be positive",
            ));
        }
        let gamma = (alpha * alpha - beta * beta).sqrt();
        let var_day = sigma_ann * sigma_ann / 365.0;
        let delta_1d = var_day * gamma.powi(3) / (alpha * alpha);
        let dt = delta_1d * step_days;
        let omega = gamma - (alpha * alpha - (beta + 1.0).powi(2)).sqrt();
        let drift = -dt * omega;
        let mean = drift + dt * beta / gamma;
        let variance = dt * alpha * alpha / gamma.powi(3);
        if !(variance.is_finite() && variance > 0.0) {
            return Err(HedgedAutocallError::Numerical(
                "step variance must be positive",
            ));
        }
        Ok(Self {
            alpha,
            beta,
            gamma,
            drift,
            dt,
            variance,
            mean,
        })
    }

    fn cf(&self, u: f64) -> Complex64 {
        let inner = complex_sqrt(
            self.alpha * self.alpha - self.beta * self.beta + u * u,
            -2.0 * self.beta * u,
        );
        let exp_re = self.dt * (self.gamma - inner.re);
        let exp_im = u * self.drift - self.dt * inner.im;
        complex_exp(exp_re, exp_im)
    }
}

#[derive(Debug, Clone)]
struct TransitionKernel {
    half_width: usize,
    offsets: Vec<isize>,
    weights: Vec<f64>,
    variance: f64,
}

#[derive(Debug, Clone)]
struct LogGrid {
    log_spots: Vec<f64>,
    spot_ratios: Vec<f64>,
    atm_index: usize,
    log_step: f64,
}

impl LogGrid {
    fn build(model: &PricingModel) -> Result<Self, HedgedAutocallError> {
        let atm_index = model.grid_points / 2;
        let mut log_spots = Vec::with_capacity(model.grid_points);
        let mut spot_ratios = Vec::with_capacity(model.grid_points);
        for idx in 0..model.grid_points {
            let offset = idx as isize - atm_index as isize;
            let log_s = offset as f64 * model.log_step;
            log_spots.push(log_s);
            spot_ratios.push(log_s.exp());
        }
        Ok(Self {
            log_spots,
            spot_ratios,
            atm_index,
            log_step: model.log_step,
        })
    }
}

fn complex_sqrt(re: f64, im: f64) -> Complex64 {
    let modulus = (re * re + im * im).sqrt();
    if modulus == 0.0 {
        return Complex64::new(0.0, 0.0);
    }
    let root_re = ((modulus + re) / 2.0).max(0.0).sqrt();
    let root_im = ((modulus - re) / 2.0).max(0.0).sqrt() * im.signum();
    Complex64::new(root_re, root_im)
}

fn complex_mul(a: Complex64, b: Complex64) -> Complex64 {
    Complex64::new(a.re * b.re - a.im * b.im, a.re * b.im + a.im * b.re)
}

fn complex_exp(re: f64, im: f64) -> Complex64 {
    let exp_re = re.exp();
    Complex64::new(exp_re * im.cos(), exp_re * im.sin())
}

fn build_transition_kernel(
    step_model: &NigStepModel,
    model: &PricingModel,
) -> Result<TransitionKernel, HedgedAutocallError> {
    let std = step_model.variance.sqrt();
    let half_width = ((model.kernel_std_width * std / model.log_step).ceil() as usize)
        .max(3)
        .min(model.grid_points / 2 - 1);
    let a = step_model.mean - model.truncation_std * std;
    let b = step_model.mean + model.truncation_std * std;
    let ba = b - a;
    if !(ba.is_finite() && ba > 0.0) {
        return Err(HedgedAutocallError::Numerical(
            "invalid COS truncation range",
        ));
    }

    let mut coeffs = Vec::with_capacity(model.cos_terms);
    for k in 0..model.cos_terms {
        let omega = k as f64 * PI / ba;
        let rotated = complex_mul(step_model.cf(omega), complex_exp(0.0, -omega * a));
        coeffs.push(2.0 / ba * rotated.re);
    }

    let mut offsets = Vec::with_capacity(2 * half_width + 1);
    let mut weights = Vec::with_capacity(2 * half_width + 1);
    let mut total_mass = 0.0;
    for offset in -(half_width as isize)..=(half_width as isize) {
        let x = offset as f64 * model.log_step;
        let theta = PI * (x - a) / ba;
        let mut density = 0.5 * coeffs[0];
        for (k, coeff) in coeffs.iter().enumerate().skip(1) {
            density += coeff * ((k as f64) * theta).cos();
        }
        let weight = (density * model.log_step).max(0.0);
        offsets.push(offset);
        weights.push(weight);
        total_mass += weight;
    }

    if !(total_mass.is_finite() && total_mass > 0.0) {
        return Err(HedgedAutocallError::Numerical(
            "transition kernel mass must be positive",
        ));
    }
    for weight in &mut weights {
        *weight /= total_mass;
    }
    Ok(TransitionKernel {
        half_width,
        offsets,
        weights,
        variance: step_model.variance,
    })
}

fn observation_days(terms: &AutocallTerms) -> Vec<usize> {
    let mut days = Vec::new();
    let mut next = terms.observation_interval_days;
    while next < terms.maturity_days {
        days.push(next);
        next += terms.observation_interval_days;
    }
    if days.last().copied() != Some(terms.maturity_days) {
        days.push(terms.maturity_days);
    }
    days
}

fn bridge_touch_probability(
    log_start: f64,
    log_end: f64,
    log_barrier: f64,
    variance: f64,
    tail_factor: f64,
) -> f64 {
    if log_start <= log_barrier || log_end <= log_barrier {
        return 1.0;
    }
    let adjusted_variance = variance * tail_factor;
    if adjusted_variance <= 0.0 {
        return 0.0;
    }
    let exponent = -2.0 * (log_start - log_barrier) * (log_end - log_barrier) / adjusted_variance;
    exponent.exp().clamp(0.0, 1.0)
}

fn compute_deltas(spot_ratios: &[f64], values: &[f64]) -> Vec<f64> {
    let mut deltas = vec![0.0; values.len()];
    for idx in 0..values.len() {
        deltas[idx] = if idx == 0 {
            (values[1] - values[0]) / (spot_ratios[1] - spot_ratios[0])
        } else if idx + 1 == values.len() {
            (values[idx] - values[idx - 1]) / (spot_ratios[idx] - spot_ratios[idx - 1])
        } else {
            (values[idx + 1] - values[idx - 1]) / (spot_ratios[idx + 1] - spot_ratios[idx - 1])
        };
    }
    deltas
}

fn interpolate(spot_ratios: &[f64], values: &[f64], spot_ratio: f64) -> f64 {
    if spot_ratio <= spot_ratios[0] {
        return values[0];
    }
    if spot_ratio >= *spot_ratios.last().unwrap_or(&spot_ratios[0]) {
        return *values.last().unwrap_or(&values[0]);
    }
    let upper = match spot_ratios.binary_search_by(|probe| probe.total_cmp(&spot_ratio)) {
        Ok(idx) => idx,
        Err(idx) => idx,
    };
    let lower = upper.saturating_sub(1);
    let left_spot = spot_ratios[lower];
    let right_spot = spot_ratios[upper];
    let left_val = values[lower];
    let right_val = values[upper];
    let weight = (spot_ratio - left_spot) / (right_spot - left_spot);
    left_val * (1.0 - weight) + right_val * weight
}

fn visible_state(
    spot_ratio: f64,
    autocalled: bool,
    knock_in_latched: bool,
    terms: &AutocallTerms,
) -> VisibleState {
    if autocalled {
        return VisibleState::Autocalled;
    }
    if spot_ratio <= terms.knock_in_barrier {
        return VisibleState::KnockInTriggeredZone;
    }
    if spot_ratio >= terms.coupon_barrier {
        return VisibleState::CouponZone;
    }
    if knock_in_latched {
        VisibleState::NoCouponKnockInLatchedZone
    } else {
        VisibleState::NoCouponNoKnockInZone
    }
}

fn run_backward_pass(
    terms: &AutocallTerms,
    model: &PricingModel,
    grid: &LogGrid,
    kernel: &TransitionKernel,
    observation_days: &HashSet<usize>,
    coupon_per_observation: f64,
) -> Result<(Vec<DailySurface>, RuntimeDiagnostics), HedgedAutocallError> {
    let log_barrier = terms.knock_in_barrier.ln();
    let n = grid.spot_ratios.len();
    let mut convolution_ops = 0usize;
    let mut bridge_ops = 0usize;

    let mut surfaces = vec![
        DailySurface {
            day: 0,
            spot_ratios: grid.spot_ratios.clone(),
            untouched_values: vec![0.0; n],
            touched_values: vec![0.0; n],
            untouched_deltas: vec![0.0; n],
            touched_deltas: vec![0.0; n],
        };
        terms.maturity_days + 1
    ];

    let mut untouched_next = vec![0.0; n];
    let mut touched_next = vec![0.0; n];
    for idx in 0..n {
        let spot_ratio = grid.spot_ratios[idx];
        let coupon = if spot_ratio >= terms.coupon_barrier {
            coupon_per_observation * terms.notional
        } else {
            0.0
        };
        let touched_redemption = if spot_ratio < 1.0 {
            terms.notional * spot_ratio
        } else {
            terms.notional
        };
        touched_next[idx] = touched_redemption + coupon;
        untouched_next[idx] = if spot_ratio <= terms.knock_in_barrier {
            touched_next[idx]
        } else {
            terms.notional + coupon
        };
    }
    surfaces[terms.maturity_days] = DailySurface {
        day: terms.maturity_days,
        spot_ratios: grid.spot_ratios.clone(),
        untouched_deltas: compute_deltas(&grid.spot_ratios, &untouched_next),
        touched_deltas: compute_deltas(&grid.spot_ratios, &touched_next),
        untouched_values: untouched_next.clone(),
        touched_values: touched_next.clone(),
    };

    for day in (0..terms.maturity_days).rev() {
        let mut untouched_today = vec![0.0; n];
        let mut touched_today = vec![0.0; n];
        let observation_day = observation_days.contains(&day);

        for j in 0..n {
            let mut expected_touched = 0.0;
            let mut expected_untouched = 0.0;
            let log_start = grid.log_spots[j];
            for (offset, weight) in kernel.offsets.iter().zip(kernel.weights.iter()) {
                let k = (j as isize + *offset).clamp(0, (n - 1) as isize) as usize;
                let log_end = grid.log_spots[k];
                let touch_probability = bridge_touch_probability(
                    log_start,
                    log_end,
                    log_barrier,
                    kernel.variance,
                    model.bridge_tail_factor,
                );
                expected_touched += weight * touched_next[k];
                expected_untouched += weight
                    * ((1.0 - touch_probability) * untouched_next[k]
                        + touch_probability * touched_next[k]);
                convolution_ops += 2;
                bridge_ops += 1;
            }

            let spot_ratio = grid.spot_ratios[j];
            let coupon = if observation_day && spot_ratio >= terms.coupon_barrier {
                coupon_per_observation * terms.notional
            } else {
                0.0
            };
            if observation_day && spot_ratio >= terms.autocall_barrier {
                let redemption = terms.notional + coupon;
                untouched_today[j] = redemption;
                touched_today[j] = redemption;
                continue;
            }
            let untouched_base = if spot_ratio <= terms.knock_in_barrier {
                expected_touched
            } else {
                expected_untouched
            };
            untouched_today[j] = untouched_base + coupon;
            touched_today[j] = expected_touched + coupon;
        }

        let untouched_deltas = compute_deltas(&grid.spot_ratios, &untouched_today);
        let touched_deltas = compute_deltas(&grid.spot_ratios, &touched_today);
        surfaces[day] = DailySurface {
            day,
            spot_ratios: grid.spot_ratios.clone(),
            untouched_values: untouched_today.clone(),
            touched_values: touched_today.clone(),
            untouched_deltas,
            touched_deltas,
        };
        untouched_next = untouched_today;
        touched_next = touched_today;
    }

    let kernel_mass = kernel.weights.iter().sum::<f64>();
    let delta_ops = surfaces
        .iter()
        .map(|surface| surface.untouched_deltas.len() + surface.touched_deltas.len())
        .sum::<usize>();
    Ok((
        surfaces,
        RuntimeDiagnostics {
            grid_points: n,
            log_step: grid.log_step,
            day_steps: terms.maturity_days,
            observation_days: observation_days.iter().copied().collect(),
            kernel_half_width: kernel.half_width,
            kernel_mass,
            convolution_ops,
            bridge_ops,
            delta_ops,
        },
    ))
}

pub fn price_hedged_autocall(
    terms: &AutocallTerms,
    model: &PricingModel,
) -> Result<PricedAutocall, HedgedAutocallError> {
    terms.validate()?;
    model.validate()?;
    let parity = price_autocall_v2_parity(terms, model)?;

    Ok(PricedAutocall {
        terms: terms.clone(),
        model: model.clone(),
        pricing: parity.pricing,
        observation_days: parity.observation_days,
        surfaces: parity.surfaces,
    })
}

impl PricedAutocall {
    pub fn hedge_decision(
        &self,
        policy: &HedgePolicy,
        day: usize,
        spot: f64,
        knock_in_latched: bool,
        autocalled: bool,
        previous_hedge_ratio: f64,
        observation_day: bool,
        previous_spot_ratio: Option<f64>,
    ) -> Result<HedgeDecision, HedgedAutocallError> {
        policy.validate()?;
        let visible = visible_state(
            spot / self.terms.entry_level,
            autocalled,
            knock_in_latched,
            &self.terms,
        );
        let spot_ratio = spot / self.terms.entry_level;
        let days_to_next_observation = self
            .observation_days
            .iter()
            .copied()
            .find(|obs_day| *obs_day >= day)
            .map(|obs_day| obs_day.saturating_sub(day))
            .unwrap_or(0);
        let raw_delta = if autocalled {
            0.0
        } else {
            let surface = self
                .surfaces
                .get(day)
                .ok_or(HedgedAutocallError::InvalidPath("day out of range"))?;
            let delta = if knock_in_latched {
                interpolate(&surface.spot_ratios, &surface.touched_deltas, spot_ratio)
            } else {
                interpolate(&surface.spot_ratios, &surface.untouched_deltas, spot_ratio)
            };
            delta.max(0.0)
        };
        let mut controller_state = HedgeControllerState::default();
        controller_state.hedge_inventory_sol = previous_hedge_ratio * self.terms.notional;
        let controller_decision = evaluate_hedge_action(
            &policy.controller_config(),
            &controller_state,
            HedgeControllerInput {
                day,
                spot,
                spot_ratio,
                note_notional: self.terms.notional,
                raw_target_delta: raw_delta,
                visible_state: visible,
                observation_day,
                coupon_barrier: self.terms.coupon_barrier,
                autocall_barrier: self.terms.autocall_barrier,
                knock_in_barrier: self.terms.knock_in_barrier,
                days_to_next_observation,
                previous_spot_ratio,
            },
        )?;
        Ok(HedgeDecision {
            target_hedge_ratio: controller_decision.executed_hedge_delta,
            raw_delta: controller_decision.raw_target_delta,
            clipped_delta: controller_decision.clipped_target_delta,
            policy_target_delta: controller_decision.policy_target_delta,
            rebalance_action: controller_decision.trade_quantity_sol / self.terms.notional,
            turnover: controller_decision.turnover,
            fee_cost: controller_decision
                .execution
                .map_or(0.0, |execution| execution.fee_cost_usdc),
            slippage_cost: controller_decision
                .execution
                .map_or(0.0, |execution| execution.slippage_cost_usdc),
            keeper_cost: controller_decision
                .execution
                .map_or(0.0, |execution| execution.keeper_cost_usdc),
            execution_cost_total: controller_decision
                .execution
                .map_or(0.0, |execution| execution.total_cost_usdc),
            trade_notional_usdc: controller_decision.trade_notional_usdc,
            visible_state: visible,
            tracking_error: controller_decision.tracking_error,
            trade_count: usize::from(controller_decision.trade_quantity_sol.abs() > 0.0),
            missed_trade: controller_decision.missed_trade,
            trigger_reason: trigger_reason_str(controller_decision.trigger_reason),
            block_reason: block_reason_str(controller_decision.block_reason),
        })
    }

    pub fn simulate_path(
        &self,
        policy: &HedgePolicy,
        path: &[PathPoint],
    ) -> Result<PathSimulationResult, HedgedAutocallError> {
        policy.validate()?;
        if path.len() != self.terms.maturity_days + 1 {
            return Err(HedgedAutocallError::InvalidPath(
                "path length must equal maturity_days + 1",
            ));
        }
        if path.first().map(|point| point.day) != Some(0) {
            return Err(HedgedAutocallError::InvalidPath("path must start at day 0"));
        }
        for window in path.windows(2) {
            if window[1].day != window[0].day + 1 {
                return Err(HedgedAutocallError::InvalidPath(
                    "path days must advance by one",
                ));
            }
            if !(window[1].close.is_finite()
                && window[1].close > 0.0
                && window[1].low.is_finite()
                && window[1].low > 0.0)
            {
                return Err(HedgedAutocallError::InvalidPath(
                    "path prices must be positive",
                ));
            }
            if window[1].low > window[1].close {
                return Err(HedgedAutocallError::InvalidPath(
                    "day low must be less than or equal to day close",
                ));
            }
        }

        let observation_days: HashSet<usize> = self.observation_days.iter().copied().collect();
        let ordered_observation_days = &self.observation_days;
        let mut steps = Vec::with_capacity(self.terms.maturity_days + 1);
        let mut autocalled = false;
        let mut knock_in_latched = false;
        let mut coupon_observations_paid = 0usize;
        let mut coupon_paid_total = 0.0f64;
        let mut retained_principal = 0.0f64;
        let explicit_margin = self.pricing.explicit_margin;
        let mut redemption_paid = 0.0f64;
        let reserve = self.pricing.reserve_need.max(
            self.pricing.quoted_coupon_per_observation
                * self.observation_days.len() as f64
                * self.terms.notional,
        );
        let mut controller_state = HedgeControllerState::default();
        let mut hedge_cash_usdc = 0.0f64;
        let mut hedge_cash_gross_usdc = 0.0f64;
        let mut coupon_cash_usdc = reserve;
        let mut peak_hedge_capital_draw = 0.0f64;
        let mut peak_coupon_capital_draw = 0.0f64;
        let mut execution_fee_total = 0.0f64;
        let mut execution_slippage_total = 0.0f64;
        let mut keeper_cost_total = 0.0f64;
        let mut capital_stack = CapitalStack::new(
            policy.capital_architecture(),
            self.terms.notional,
            reserve,
            explicit_margin,
        )?;

        let initial_decision =
            self.hedge_decision(policy, 0, path[0].close, false, false, 0.0, false, None)?;
        if initial_decision.rebalance_action.abs() > 0.0 {
            let trade_notional =
                initial_decision.rebalance_action * self.terms.notional * path[0].close;
            hedge_cash_gross_usdc -= trade_notional;
            hedge_cash_usdc -= trade_notional;
            hedge_cash_usdc -= initial_decision.execution_cost_total;
            controller_state.hedge_inventory_sol +=
                initial_decision.rebalance_action * self.terms.notional;
            controller_state.trade_count += 1;
            controller_state.total_turnover += initial_decision.turnover;
            controller_state.total_execution_cost_usdc += initial_decision.execution_cost_total;
            controller_state.total_keeper_cost_usdc += initial_decision.keeper_cost;
            controller_state.last_rebalance_day = Some(0);
            controller_state.rebalances_today = 1;
            execution_fee_total += initial_decision.fee_cost;
            execution_slippage_total += initial_decision.slippage_cost;
            keeper_cost_total += initial_decision.keeper_cost;
        }
        capital_stack.set_hedge_position(controller_state.hedge_inventory_sol, hedge_cash_usdc);
        peak_hedge_capital_draw = peak_hedge_capital_draw.max((-hedge_cash_usdc).max(0.0));
        let initial_committed = capital_stack.reserve_occupancy();
        steps.push(SimulationStep {
            day: 0,
            close_ratio: path[0].close / self.terms.entry_level,
            low_ratio: path[0].low / self.terms.entry_level,
            observation_day: false,
            visible_state: initial_decision.visible_state,
            hedge_decision: initial_decision,
            coupon_paid: 0.0,
            autocalled: false,
            hedge_inventory_sol: controller_state.hedge_inventory_sol,
            hedge_cash_usdc,
            coupon_outflows_cumulative: 0.0,
            reserve_occupancy: initial_committed,
        });

        for point in path.iter().skip(1) {
            let day = point.day;
            let close_ratio = point.close / self.terms.entry_level;
            let low_ratio = point.low / self.terms.entry_level;
            let observation_day = observation_days.contains(&day);
            let mut coupon_paid = 0.0f64;
            if observation_day && close_ratio <= self.terms.knock_in_barrier {
                knock_in_latched = true;
            }

            if !autocalled && observation_day && close_ratio >= self.terms.coupon_barrier {
                coupon_paid = self.pricing.quoted_coupon_per_observation * self.terms.notional;
                coupon_paid_total += coupon_paid;
                coupon_observations_paid += 1;
                coupon_cash_usdc -= coupon_paid;
                capital_stack.accrue_coupon(coupon_paid);
            }

            // Observation index (1-indexed): obs 1 = first observation (day 2), etc.
            let obs_index = if observation_day {
                ordered_observation_days
                    .binary_search(&day)
                    .map(|i| i + 1)
                    .unwrap_or(0)
            } else {
                0
            };
            let autocall_allowed =
                observation_day && obs_index > self.terms.no_autocall_first_n_obs;

            if !autocalled && autocall_allowed && close_ratio >= self.terms.autocall_barrier {
                autocalled = true;
                redemption_paid = self.terms.notional;
                retained_principal = 0.0;
            } else if !autocalled && day == self.terms.maturity_days {
                redemption_paid = if knock_in_latched && close_ratio < 1.0 {
                    self.terms.notional * close_ratio
                } else {
                    self.terms.notional
                };
                retained_principal = self.terms.notional - redemption_paid;
            }

            let decision = if autocalled || day == self.terms.maturity_days {
                exit_decision(
                    policy,
                    self.terms.notional,
                    point.close,
                    controller_state.hedge_inventory_sol,
                    visible_state(close_ratio, autocalled, knock_in_latched, &self.terms),
                )?
            } else {
                self.hedge_decision(
                    policy,
                    day,
                    point.close,
                    knock_in_latched,
                    false,
                    controller_state.hedge_inventory_sol / self.terms.notional,
                    observation_day,
                    path.get(day.saturating_sub(1))
                        .map(|previous| previous.close),
                )?
            };

            if day != 0 && controller_state.last_rebalance_day != Some(day) {
                controller_state.rebalances_today = 0;
            }
            if decision.rebalance_action.abs() > 0.0 {
                let trade_quantity_sol = decision.rebalance_action * self.terms.notional;
                let trade_notional = trade_quantity_sol * point.close;
                hedge_cash_gross_usdc -= trade_notional;
                hedge_cash_usdc -= trade_notional;
                hedge_cash_usdc -= decision.execution_cost_total;
                controller_state.hedge_inventory_sol += trade_quantity_sol;
                controller_state.trade_count += 1;
                controller_state.total_turnover += decision.turnover;
                controller_state.total_execution_cost_usdc += decision.execution_cost_total;
                controller_state.total_keeper_cost_usdc += decision.keeper_cost;
                controller_state.last_rebalance_day = Some(day);
                controller_state.rebalances_today += 1;
                execution_fee_total += decision.fee_cost;
                execution_slippage_total += decision.slippage_cost;
                keeper_cost_total += decision.keeper_cost;
            }
            if decision.missed_trade {
                controller_state.missed_trades += 1;
            }
            capital_stack.set_hedge_position(controller_state.hedge_inventory_sol, hedge_cash_usdc);

            if autocalled || day == self.terms.maturity_days {
                if retained_principal != 0.0 {
                    coupon_cash_usdc += retained_principal;
                    capital_stack.settle_retained_principal(retained_principal);
                }
            }

            peak_hedge_capital_draw = peak_hedge_capital_draw.max((-hedge_cash_usdc).max(0.0));
            peak_coupon_capital_draw =
                peak_coupon_capital_draw.max((reserve - coupon_cash_usdc).max(0.0));
            let reserve_occupancy = capital_stack.reserve_occupancy();

            steps.push(SimulationStep {
                day,
                close_ratio,
                low_ratio,
                observation_day,
                visible_state: decision.visible_state,
                hedge_decision: decision,
                coupon_paid,
                autocalled,
                hedge_inventory_sol: controller_state.hedge_inventory_sol,
                hedge_cash_usdc,
                coupon_outflows_cumulative: coupon_paid_total,
                reserve_occupancy,
            });

            if autocalled {
                break;
            }
        }

        let hedge_gross_pnl = hedge_cash_gross_usdc;
        let hedge_pnl = hedge_cash_usdc;
        let pnl_components = PnlComponents {
            hedge_gross_pnl,
            swap_fee_cost: execution_fee_total,
            slippage_cost: execution_slippage_total,
            keeper_cost: keeper_cost_total,
            coupon_funding_cost: coupon_paid_total,
            reserve_carry: 0.0,
            knock_in_residual: retained_principal,
            explicit_margin,
            net_vault_pnl: retained_principal + explicit_margin + hedge_gross_pnl
                - execution_fee_total
                - execution_slippage_total
                - keeper_cost_total
                - coupon_paid_total,
        };
        let net_vault_pnl = pnl_components.net_vault_pnl;
        let buyer_total_return =
            (coupon_paid_total + redemption_paid - self.terms.notional) / self.terms.notional;
        let buyer_annualized_return =
            (1.0 + buyer_total_return).powf(365.0 / self.terms.maturity_days as f64) - 1.0;
        let accounting_snapshot = capital_stack.snapshot(net_vault_pnl);

        Ok(PathSimulationResult {
            buyer_total_return,
            buyer_annualized_return,
            profitable_note: buyer_total_return > 0.0,
            autocalled,
            knock_in_triggered: knock_in_latched,
            coupon_observations_paid,
            coupon_paid_total,
            redemption_paid,
            retained_principal,
            explicit_margin,
            hedge_gross_pnl,
            hedge_pnl,
            fee_cost_total: execution_fee_total,
            keeper_cost_total,
            slippage_cost: execution_slippage_total,
            turnover: controller_state.total_turnover,
            trade_count: controller_state.trade_count,
            missed_trade_count: controller_state.missed_trades,
            execution_cost_total: controller_state.total_execution_cost_usdc,
            net_vault_pnl,
            pnl_components: pnl_components.clone(),
            accounting: AccountingState {
                escrowed_user_principal: self.terms.notional,
                hedge_inventory_sol: controller_state.hedge_inventory_sol,
                hedge_cash_usdc,
                coupon_cash_usdc,
                coupon_outflows: coupon_paid_total,
                underwriting_reserve: reserve,
                accrued_unpaid_coupons: accounting_snapshot.accrued_unpaid_coupons,
                issuer_margin_usdc: explicit_margin,
                hedge_pnl,
                net_vault_pnl,
                peak_hedge_capital_draw,
                peak_coupon_capital_draw,
                peak_committed_capital: accounting_snapshot.peak_committed_capital,
                reserve_occupancy: accounting_snapshot.reserve_occupancy,
                shortfall_flag: accounting_snapshot.shortfall_flag,
                insolvency_flag: accounting_snapshot.insolvency_flag,
                execution_fee_total,
                execution_slippage_total,
                keeper_cost_total,
                hedge_gross_pnl,
                pnl_components,
            },
            steps,
        })
    }
}

fn exit_decision(
    policy: &HedgePolicy,
    note_notional: f64,
    spot: f64,
    hedge_inventory_sol: f64,
    visible_state: VisibleState,
) -> Result<HedgeDecision, HedgedAutocallError> {
    let trade_quantity_sol = -hedge_inventory_sol;
    let execution = if trade_quantity_sol.abs() > 0.0 {
        crate::sol_swap_cost::estimate_swap_execution(
            spot,
            trade_quantity_sol,
            &policy.controller_config().swap_cost,
        )?
    } else {
        crate::sol_swap_cost::SolSwapExecution {
            oracle_price: spot,
            execution_price: spot,
            trade_notional_abs: 0.0,
            trade_quantity_sol: 0.0,
            fee_cost_usdc: 0.0,
            slippage_cost_usdc: 0.0,
            keeper_cost_usdc: 0.0,
            total_cost_usdc: 0.0,
            total_cost_bps: 0.0,
        }
    };
    Ok(HedgeDecision {
        target_hedge_ratio: 0.0,
        raw_delta: 0.0,
        clipped_delta: 0.0,
        policy_target_delta: 0.0,
        rebalance_action: trade_quantity_sol / note_notional,
        turnover: trade_quantity_sol.abs() / note_notional,
        fee_cost: execution.fee_cost_usdc,
        slippage_cost: execution.slippage_cost_usdc,
        keeper_cost: execution.keeper_cost_usdc,
        execution_cost_total: execution.total_cost_usdc,
        trade_notional_usdc: execution.trade_notional_abs,
        visible_state,
        tracking_error: 0.0,
        trade_count: usize::from(trade_quantity_sol.abs() > 0.0),
        missed_trade: false,
        trigger_reason: "exit",
        block_reason: "none",
    })
}

fn trigger_reason_str(reason: HedgeTriggerReason) -> &'static str {
    match reason {
        HedgeTriggerReason::Initial => "initial",
        HedgeTriggerReason::Observation => "observation",
        HedgeTriggerReason::DeltaBand => "delta_band",
        HedgeTriggerReason::Static => "static",
        HedgeTriggerReason::None => "none",
    }
}

fn block_reason_str(reason: HedgeBlockReason) -> &'static str {
    match reason {
        HedgeBlockReason::None => "none",
        HedgeBlockReason::AutocalledState => "autocalled_state",
        HedgeBlockReason::Cooldown => "cooldown",
        HedgeBlockReason::BelowMinTrade => "below_min_trade",
        HedgeBlockReason::FrequencyCap => "frequency_cap",
        HedgeBlockReason::CostThreshold => "cost_threshold",
    }
}

fn apply_hedge_trade(
    terms: &AutocallTerms,
    spot: f64,
    target_hedge_ratio: f64,
    hedge_ratio: &mut f64,
    hedge_inventory_sol: &mut f64,
    hedge_cash_usdc: &mut f64,
    turnover: &mut f64,
    slippage_cost: &mut f64,
    trade_slippage: f64,
) {
    let target_notional = target_hedge_ratio * terms.notional;
    let target_inventory = if spot > 0.0 {
        target_notional / spot
    } else {
        0.0
    };
    let delta_inventory = target_inventory - *hedge_inventory_sol;
    let trade_value = delta_inventory * spot;
    *hedge_cash_usdc -= trade_value;
    *hedge_cash_usdc -= trade_slippage;
    *hedge_inventory_sol = target_inventory;
    *hedge_ratio = target_hedge_ratio;
    *turnover += trade_value.abs() / terms.notional;
    *slippage_cost += trade_slippage;
}

fn committed_capital(
    reserve: f64,
    policy: &HedgePolicy,
    hedge_capital_draw: f64,
    coupon_capital_draw: f64,
) -> f64 {
    let hedge_component = match policy.hedge_funding_mode {
        HedgeFundingMode::SeparateHedgeSleeve => 0.0,
        HedgeFundingMode::UnderwritingVault => hedge_capital_draw,
    };
    let coupon_component = match policy.coupon_vault_mode {
        CouponVaultMode::SeparateCouponVault => 0.0,
        CouponVaultMode::SharedUnderwriting => coupon_capital_draw,
    };
    reserve + hedge_component + coupon_component
}
