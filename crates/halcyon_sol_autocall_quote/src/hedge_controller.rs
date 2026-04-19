use crate::autocall_hedged::{HedgeMode, HedgedAutocallError, VisibleState};
use crate::sol_swap_cost::{estimate_swap_execution, SolSwapCostConfig, SolSwapExecution};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HedgeTargetPolicy {
    RawDelta,
    StateCap,
    KiTaper,
    PostKiZero,
    RecoveryOnly,
    CostAware,
    CallZoneOnly,
    DownsideLadder,
    ZoneAware,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HedgeControllerConfig {
    pub hedge_mode: HedgeMode,
    pub initial_hedge_delta: f64,
    pub initial_hedge_from_model_delta: bool,
    pub delta_clip: f64,
    pub hedge_band: f64,
    pub cooldown_days: usize,
    pub min_trade_delta: f64,
    pub max_trade_delta: f64,
    pub max_rebalances_per_day: usize,
    pub force_observation_review: bool,
    pub allow_intraperiod_checks: bool,
    pub swap_cost: SolSwapCostConfig,
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
}

impl Default for HedgeControllerConfig {
    fn default() -> Self {
        Self {
            hedge_mode: HedgeMode::None,
            initial_hedge_delta: 0.0,
            initial_hedge_from_model_delta: false,
            delta_clip: 1.0,
            hedge_band: 0.05,
            cooldown_days: 0,
            min_trade_delta: 0.0,
            max_trade_delta: 1.0,
            max_rebalances_per_day: 8,
            force_observation_review: true,
            allow_intraperiod_checks: true,
            swap_cost: SolSwapCostConfig::default(),
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
        }
    }
}

impl HedgeControllerConfig {
    pub fn validate(&self) -> Result<(), HedgedAutocallError> {
        if !(0.0..=1.0).contains(&self.initial_hedge_delta) {
            return Err(HedgedAutocallError::InvalidTerms(
                "initial_hedge_delta must be in [0,1]",
            ));
        }
        if !(0.0..=1.0).contains(&self.delta_clip) || self.delta_clip == 0.0 {
            return Err(HedgedAutocallError::InvalidTerms(
                "delta_clip must be in (0,1]",
            ));
        }
        if !(0.0..=1.0).contains(&self.hedge_band) {
            return Err(HedgedAutocallError::InvalidTerms(
                "hedge_band must be in [0,1]",
            ));
        }
        if !(0.0..=1.0).contains(&self.min_trade_delta) {
            return Err(HedgedAutocallError::InvalidTerms(
                "min_trade_delta must be in [0,1]",
            ));
        }
        if !(0.0..=1.0).contains(&self.max_trade_delta) || self.max_trade_delta == 0.0 {
            return Err(HedgedAutocallError::InvalidTerms(
                "max_trade_delta must be in (0,1]",
            ));
        }
        if self.max_trade_delta < self.min_trade_delta {
            return Err(HedgedAutocallError::InvalidTerms(
                "max_trade_delta must be >= min_trade_delta",
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
        if !(self.downside_soft_threshold.is_finite()
            && self.downside_deep_threshold.is_finite()
            && self.downside_soft_threshold > 0.0
            && self.downside_soft_threshold <= 1.0
            && self.downside_deep_threshold > 0.0
            && self.downside_deep_threshold <= self.downside_soft_threshold)
        {
            return Err(HedgedAutocallError::InvalidTerms(
                "downside ladder thresholds must satisfy 0 < deep <= soft <= 1",
            ));
        }
        if !(0.0..=1.0).contains(&self.downside_soft_cap) {
            return Err(HedgedAutocallError::InvalidTerms(
                "downside_soft_cap must be in [0,1]",
            ));
        }
        if !(0.0..=1.0).contains(&self.downside_deep_cap) {
            return Err(HedgedAutocallError::InvalidTerms(
                "downside_deep_cap must be in [0,1]",
            ));
        }
        if self.downside_deep_cap > self.downside_soft_cap {
            return Err(HedgedAutocallError::InvalidTerms(
                "downside_deep_cap must be <= downside_soft_cap",
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
        self.swap_cost.validate()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HedgeControllerState {
    pub hedge_inventory_sol: f64,
    pub last_rebalance_day: Option<usize>,
    pub rebalances_today: usize,
    pub total_turnover: f64,
    pub trade_count: usize,
    pub missed_trades: usize,
    pub total_execution_cost_usdc: f64,
    pub total_keeper_cost_usdc: f64,
}

impl Default for HedgeControllerState {
    fn default() -> Self {
        Self {
            hedge_inventory_sol: 0.0,
            last_rebalance_day: None,
            rebalances_today: 0,
            total_turnover: 0.0,
            trade_count: 0,
            missed_trades: 0,
            total_execution_cost_usdc: 0.0,
            total_keeper_cost_usdc: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HedgeControllerInput {
    pub day: usize,
    pub spot: f64,
    pub spot_ratio: f64,
    pub note_notional: f64,
    pub raw_target_delta: f64,
    pub visible_state: VisibleState,
    pub observation_day: bool,
    pub coupon_barrier: f64,
    pub autocall_barrier: f64,
    pub knock_in_barrier: f64,
    pub days_to_next_observation: usize,
    pub previous_spot_ratio: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HedgeTriggerReason {
    Initial,
    Observation,
    DeltaBand,
    Static,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HedgeBlockReason {
    None,
    AutocalledState,
    Cooldown,
    BelowMinTrade,
    FrequencyCap,
    CostThreshold,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HedgeControllerDecision {
    pub raw_target_delta: f64,
    pub clipped_target_delta: f64,
    pub policy_target_delta: f64,
    pub current_hedge_delta: f64,
    pub desired_hedge_delta: f64,
    pub executed_hedge_delta: f64,
    pub delta_gap: f64,
    pub turnover: f64,
    pub tracking_error: f64,
    pub trade_quantity_sol: f64,
    pub trade_notional_usdc: f64,
    pub trigger_reason: HedgeTriggerReason,
    pub block_reason: HedgeBlockReason,
    pub missed_trade: bool,
    pub expected_benefit_usdc: f64,
    pub estimated_cost_usdc: f64,
    pub execution: Option<SolSwapExecution>,
}

fn clipped_target(raw_target_delta: f64, clip: f64) -> f64 {
    raw_target_delta.max(0.0).min(clip)
}

fn smoothstep01(value: f64) -> f64 {
    let x = value.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn active_call_zone(input: HedgeControllerInput, buffer: f64) -> bool {
    input.observation_day
        || input.spot_ratio >= (input.coupon_barrier - buffer).max(input.knock_in_barrier)
        || (input.autocall_barrier - input.spot_ratio).abs() <= buffer
}

fn downside_ladder_target(
    config: &HedgeControllerConfig,
    input: HedgeControllerInput,
    clipped: f64,
) -> f64 {
    if matches!(
        input.visible_state,
        VisibleState::NoCouponKnockInLatchedZone | VisibleState::KnockInTriggeredZone
    ) {
        return clipped.min(config.post_ki_delta_cap);
    }
    if input.spot_ratio > config.downside_soft_threshold {
        clipped
    } else if input.spot_ratio > config.downside_deep_threshold {
        clipped.min(config.downside_soft_cap)
    } else {
        clipped.min(config.downside_deep_cap)
    }
}

fn state_cap_target(
    config: &HedgeControllerConfig,
    input: HedgeControllerInput,
    clipped: f64,
) -> f64 {
    let cap = match input.visible_state {
        VisibleState::Autocalled => 0.0,
        VisibleState::CouponZone => config.coupon_zone_delta_cap,
        VisibleState::NoCouponNoKnockInZone => config.sub_coupon_delta_cap,
        VisibleState::NoCouponKnockInLatchedZone | VisibleState::KnockInTriggeredZone => {
            config.post_ki_delta_cap
        }
    };
    clipped.min(cap)
}

fn ki_taper_target(
    config: &HedgeControllerConfig,
    input: HedgeControllerInput,
    clipped: f64,
) -> f64 {
    if matches!(
        input.visible_state,
        VisibleState::NoCouponKnockInLatchedZone | VisibleState::KnockInTriggeredZone
    ) {
        return clipped.min(config.post_ki_delta_cap);
    }
    if input.spot_ratio >= input.coupon_barrier {
        return clipped.min(config.coupon_zone_delta_cap);
    }
    if input.spot_ratio <= input.knock_in_barrier {
        return clipped.min(config.post_ki_delta_cap.max(config.ki_taper_floor));
    }
    let width = (input.coupon_barrier - input.knock_in_barrier).max(1e-9);
    let progress = ((input.spot_ratio - input.knock_in_barrier) / width).clamp(0.0, 1.0);
    let taper = config.ki_taper_floor + (1.0 - config.ki_taper_floor) * progress;
    (clipped * taper)
        .min(config.sub_coupon_delta_cap)
        .min(config.coupon_zone_delta_cap)
}

fn zone_aware_target(
    config: &HedgeControllerConfig,
    input: HedgeControllerInput,
    clipped: f64,
) -> f64 {
    if matches!(input.visible_state, VisibleState::Autocalled) {
        return 0.0;
    }
    if matches!(
        input.visible_state,
        VisibleState::NoCouponKnockInLatchedZone | VisibleState::KnockInTriggeredZone
    ) {
        return clipped.min(config.post_ki_delta_cap);
    }

    let barrier_width = (input.coupon_barrier - input.knock_in_barrier).max(1e-9);
    let ki_proximity = ((input.coupon_barrier - input.spot_ratio) / barrier_width).clamp(0.0, 1.0);
    let base_lambda = config.coupon_zone_floor_lambda
        + (config.near_ki_lambda - config.coupon_zone_floor_lambda) * smoothstep01(ki_proximity);

    let near_observation = config.observation_hysteresis_days > 0
        && input.days_to_next_observation <= config.observation_hysteresis_days;
    let observation_weight = if near_observation {
        (-(input.days_to_next_observation as f64)
            / (config.observation_hysteresis_days as f64).max(1.0))
        .exp()
    } else {
        0.0
    };
    let downside_gap = ((input.coupon_barrier - input.spot_ratio) / input.coupon_barrier.max(1e-9))
        .clamp(0.0, 1.0);
    let observation_lambda = config.coupon_zone_floor_lambda
        + (config.near_ki_lambda - config.coupon_zone_floor_lambda)
            * observation_weight
            * smoothstep01(downside_gap);

    let lambda = base_lambda.max(observation_lambda).clamp(0.0, 1.0);
    (clipped * lambda).clamp(0.0, clipped)
}

fn policy_target(config: &HedgeControllerConfig, input: HedgeControllerInput, clipped: f64) -> f64 {
    match config.target_policy {
        HedgeTargetPolicy::RawDelta => clipped,
        HedgeTargetPolicy::StateCap => state_cap_target(config, input, clipped),
        HedgeTargetPolicy::KiTaper => ki_taper_target(config, input, clipped),
        HedgeTargetPolicy::PostKiZero => {
            if matches!(
                input.visible_state,
                VisibleState::NoCouponKnockInLatchedZone | VisibleState::KnockInTriggeredZone
            ) {
                0.0
            } else {
                ki_taper_target(config, input, clipped)
            }
        }
        HedgeTargetPolicy::RecoveryOnly => {
            if matches!(
                input.visible_state,
                VisibleState::NoCouponKnockInLatchedZone | VisibleState::KnockInTriggeredZone
            ) {
                if input.spot_ratio >= config.recovery_reentry_ratio {
                    clipped.min(config.recovery_reentry_delta_cap)
                } else {
                    0.0
                }
            } else {
                ki_taper_target(config, input, clipped)
            }
        }
        HedgeTargetPolicy::CostAware => state_cap_target(config, input, clipped),
        HedgeTargetPolicy::CallZoneOnly => {
            if matches!(
                input.visible_state,
                VisibleState::NoCouponKnockInLatchedZone | VisibleState::KnockInTriggeredZone
            ) {
                0.0
            } else if active_call_zone(input, config.call_zone_buffer) {
                clipped.min(config.coupon_zone_delta_cap)
            } else {
                0.0
            }
        }
        HedgeTargetPolicy::DownsideLadder => downside_ladder_target(config, input, clipped),
        HedgeTargetPolicy::ZoneAware => zone_aware_target(config, input, clipped),
    }
}

fn effective_hedge_band(config: &HedgeControllerConfig, input: HedgeControllerInput) -> f64 {
    let mut band = config.hedge_band;
    if matches!(config.target_policy, HedgeTargetPolicy::ZoneAware)
        && matches!(input.visible_state, VisibleState::CouponZone)
    {
        band *= config.coupon_zone_band_multiplier;
        if config.observation_hysteresis_days > 0
            && input.days_to_next_observation <= config.observation_hysteresis_days
        {
            band *= 1.25;
        }
    }
    band.clamp(0.0, 1.0)
}

fn apply_zone_aware_hysteresis(
    config: &HedgeControllerConfig,
    input: HedgeControllerInput,
    current_hedge_delta: f64,
    desired: f64,
) -> f64 {
    if !matches!(config.target_policy, HedgeTargetPolicy::ZoneAware) {
        return desired;
    }
    let near_observation = config.observation_hysteresis_days > 0
        && input.days_to_next_observation <= config.observation_hysteresis_days;
    let upside_move = input
        .previous_spot_ratio
        .is_some_and(|previous| input.spot_ratio > previous + 1e-12);
    let mut adjusted = desired;

    if near_observation && upside_move && input.spot_ratio >= input.coupon_barrier {
        adjusted = adjusted.min(current_hedge_delta);
    }

    if config.rebound_unwind_half_life_days > 0.0 && upside_move && adjusted < current_hedge_delta {
        let decay_fraction =
            1.0 - 0.5f64.powf(1.0 / config.rebound_unwind_half_life_days.max(1e-6));
        let min_allowed_delta = current_hedge_delta * (1.0 - decay_fraction);
        adjusted = adjusted.max(min_allowed_delta);
    }

    adjusted
}

pub fn evaluate_hedge_action(
    config: &HedgeControllerConfig,
    state: &HedgeControllerState,
    input: HedgeControllerInput,
) -> Result<HedgeControllerDecision, HedgedAutocallError> {
    config.validate()?;
    if !(input.spot.is_finite() && input.spot > 0.0) {
        return Err(HedgedAutocallError::InvalidPath("spot must be positive"));
    }
    if !(input.note_notional.is_finite() && input.note_notional > 0.0) {
        return Err(HedgedAutocallError::InvalidPath(
            "note_notional must be positive",
        ));
    }

    let current_hedge_delta = state.hedge_inventory_sol / input.note_notional;
    let clipped = clipped_target(input.raw_target_delta, config.delta_clip);
    let policy_target_delta = policy_target(config, input, clipped);
    let dynamic_band = effective_hedge_band(config, input);
    let desired = match config.hedge_mode {
        HedgeMode::None => 0.0,
        HedgeMode::StaticFraction => config.initial_hedge_delta,
        HedgeMode::DeltaObservationOnly => {
            if input.day == 0 {
                if config.initial_hedge_from_model_delta {
                    policy_target_delta
                } else {
                    config.initial_hedge_delta
                }
            } else if input.observation_day {
                policy_target_delta
            } else {
                current_hedge_delta
            }
        }
        HedgeMode::DeltaDaily => {
            if input.day == 0 {
                if config.initial_hedge_from_model_delta {
                    policy_target_delta
                } else {
                    config.initial_hedge_delta
                }
            } else if config.allow_intraperiod_checks || input.observation_day {
                policy_target_delta
            } else {
                current_hedge_delta
            }
        }
        HedgeMode::DeltaObservationPlusThreshold => {
            if input.day == 0 {
                if config.initial_hedge_from_model_delta {
                    policy_target_delta
                } else {
                    config.initial_hedge_delta
                }
            } else if input.observation_day
                || (config.allow_intraperiod_checks
                    && (policy_target_delta - current_hedge_delta).abs() >= dynamic_band)
            {
                policy_target_delta
            } else {
                current_hedge_delta
            }
        }
    };
    let desired = apply_zone_aware_hysteresis(config, input, current_hedge_delta, desired);

    let target_gap = desired - current_hedge_delta;
    let trigger_reason = if input.day == 0 {
        HedgeTriggerReason::Initial
    } else if matches!(config.hedge_mode, HedgeMode::StaticFraction) {
        HedgeTriggerReason::Static
    } else if input.observation_day && config.force_observation_review {
        HedgeTriggerReason::Observation
    } else if (policy_target_delta - current_hedge_delta).abs() >= dynamic_band {
        HedgeTriggerReason::DeltaBand
    } else {
        HedgeTriggerReason::None
    };

    let estimated_cost_usdc = if target_gap.abs() > 0.0 {
        estimate_swap_execution(
            input.spot,
            target_gap * input.note_notional,
            &config.swap_cost,
        )?
        .total_cost_usdc
    } else {
        0.0
    };
    let expected_benefit_usdc =
        target_gap.abs() * input.note_notional * input.spot * config.cost_aware_expected_move;
    let mut block_reason = HedgeBlockReason::None;
    if matches!(input.visible_state, VisibleState::Autocalled) {
        block_reason = HedgeBlockReason::AutocalledState;
    } else if config.cooldown_days > 0
        && state
            .last_rebalance_day
            .is_some_and(|last| input.day.saturating_sub(last) < config.cooldown_days)
        && target_gap.abs() > 0.0
    {
        block_reason = HedgeBlockReason::Cooldown;
    } else if state.last_rebalance_day == Some(input.day)
        && state.rebalances_today >= config.max_rebalances_per_day
        && target_gap.abs() > 0.0
    {
        block_reason = HedgeBlockReason::FrequencyCap;
    } else if matches!(config.target_policy, HedgeTargetPolicy::CostAware)
        && target_gap.abs() >= config.min_trade_delta
        && expected_benefit_usdc <= estimated_cost_usdc * config.cost_aware_threshold_multiple
    {
        block_reason = HedgeBlockReason::CostThreshold;
    } else if target_gap.abs() < config.min_trade_delta {
        block_reason = HedgeBlockReason::BelowMinTrade;
    }

    let missed_trade = target_gap.abs() > config.min_trade_delta
        && !matches!(block_reason, HedgeBlockReason::None);
    let executed_delta = if missed_trade {
        current_hedge_delta
    } else {
        let clipped_trade = target_gap.clamp(-config.max_trade_delta, config.max_trade_delta);
        current_hedge_delta + clipped_trade
    };
    let trade_delta = executed_delta - current_hedge_delta;
    let trade_quantity_sol = trade_delta * input.note_notional;
    let execution = if trade_quantity_sol.abs() > 0.0 {
        Some(estimate_swap_execution(
            input.spot,
            trade_quantity_sol,
            &config.swap_cost,
        )?)
    } else {
        None
    };
    let trade_notional_usdc = execution.map_or(0.0, |item| item.trade_notional_abs);
    let turnover = if input.note_notional > 0.0 {
        trade_quantity_sol.abs() / input.note_notional
    } else {
        0.0
    };

    Ok(HedgeControllerDecision {
        raw_target_delta: input.raw_target_delta.max(0.0),
        clipped_target_delta: clipped,
        policy_target_delta,
        current_hedge_delta,
        desired_hedge_delta: desired,
        executed_hedge_delta: executed_delta,
        delta_gap: target_gap,
        turnover,
        tracking_error: desired - executed_delta,
        trade_quantity_sol,
        trade_notional_usdc,
        trigger_reason,
        block_reason,
        missed_trade,
        expected_benefit_usdc,
        estimated_cost_usdc,
        execution,
    })
}

pub fn apply_hedge_decision(
    state: &mut HedgeControllerState,
    input: HedgeControllerInput,
    decision: &HedgeControllerDecision,
) {
    if decision.trade_quantity_sol.abs() > 0.0 {
        state.hedge_inventory_sol += decision.trade_quantity_sol;
        state.total_turnover += decision.turnover;
        state.trade_count += 1;
        state.last_rebalance_day = Some(input.day);
        if state.last_rebalance_day == Some(input.day) {
            state.rebalances_today += 1;
        }
    } else if state.last_rebalance_day != Some(input.day) {
        state.rebalances_today = 0;
    }
    if decision.missed_trade {
        state.missed_trades += 1;
    }
    if let Some(execution) = decision.execution {
        state.total_execution_cost_usdc += execution.total_cost_usdc;
        state.total_keeper_cost_usdc += execution.keeper_cost_usdc;
    }
    if state.last_rebalance_day != Some(input.day) {
        state.rebalances_today = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moderate_config() -> HedgeControllerConfig {
        HedgeControllerConfig {
            hedge_mode: HedgeMode::DeltaObservationPlusThreshold,
            initial_hedge_delta: 0.0,
            initial_hedge_from_model_delta: false,
            delta_clip: 1.0,
            hedge_band: 0.05,
            cooldown_days: 0,
            min_trade_delta: 0.01,
            max_trade_delta: 1.0,
            max_rebalances_per_day: 4,
            force_observation_review: true,
            allow_intraperiod_checks: true,
            swap_cost: SolSwapCostConfig::default(),
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
        }
    }

    #[test]
    fn observation_forces_review() {
        let decision = evaluate_hedge_action(
            &moderate_config(),
            &HedgeControllerState::default(),
            HedgeControllerInput {
                day: 2,
                spot: 120.0,
                note_notional: 1.0,
                raw_target_delta: 0.50,
                spot_ratio: 1.0,
                visible_state: VisibleState::CouponZone,
                observation_day: true,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 0,
                previous_spot_ratio: Some(1.0),
            },
        )
        .expect("decision");
        assert!(decision.trade_quantity_sol > 0.0);
        assert_eq!(decision.trigger_reason, HedgeTriggerReason::Observation);
    }

    #[test]
    fn day_zero_can_seed_from_model_delta() {
        let mut config = moderate_config();
        config.hedge_mode = HedgeMode::DeltaObservationOnly;
        config.initial_hedge_delta = 0.50;
        config.initial_hedge_from_model_delta = true;

        let decision = evaluate_hedge_action(
            &config,
            &HedgeControllerState::default(),
            HedgeControllerInput {
                day: 0,
                spot: 100.0,
                note_notional: 1.0,
                raw_target_delta: 0.32,
                spot_ratio: 1.0,
                visible_state: VisibleState::CouponZone,
                observation_day: false,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 2,
                previous_spot_ratio: None,
            },
        )
        .expect("decision");

        assert!((decision.policy_target_delta - 0.32).abs() < 1e-12);
        assert!((decision.executed_hedge_delta - 0.32).abs() < 1e-12);
        assert_ne!(decision.executed_hedge_delta, 0.50);
    }

    #[test]
    fn small_gap_is_blocked_by_min_trade() {
        let decision = evaluate_hedge_action(
            &moderate_config(),
            &HedgeControllerState {
                hedge_inventory_sol: 0.49,
                ..HedgeControllerState::default()
            },
            HedgeControllerInput {
                day: 3,
                spot: 120.0,
                note_notional: 1.0,
                raw_target_delta: 0.495,
                spot_ratio: 1.0,
                visible_state: VisibleState::CouponZone,
                observation_day: false,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 1,
                previous_spot_ratio: Some(1.0),
            },
        )
        .expect("decision");
        assert_eq!(decision.block_reason, HedgeBlockReason::BelowMinTrade);
        assert_eq!(decision.trade_quantity_sol, 0.0);
    }

    #[test]
    fn cooldown_blocks_delayed_crank_rebalance() {
        let mut config = moderate_config();
        config.cooldown_days = 3;
        let decision = evaluate_hedge_action(
            &config,
            &HedgeControllerState {
                hedge_inventory_sol: 0.10,
                last_rebalance_day: Some(2),
                rebalances_today: 1,
                ..HedgeControllerState::default()
            },
            HedgeControllerInput {
                day: 4,
                spot: 120.0,
                note_notional: 1.0,
                raw_target_delta: 0.60,
                spot_ratio: 1.0,
                visible_state: VisibleState::CouponZone,
                observation_day: true,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 0,
                previous_spot_ratio: Some(1.0),
            },
        )
        .expect("decision");
        assert_eq!(decision.block_reason, HedgeBlockReason::Cooldown);
        assert!(decision.missed_trade);
        assert_eq!(decision.trade_quantity_sol, 0.0);
    }

    #[test]
    fn state_cap_trims_sub_coupon_and_post_ki_delta() {
        let mut config = moderate_config();
        config.target_policy = HedgeTargetPolicy::StateCap;
        config.sub_coupon_delta_cap = 0.20;
        config.post_ki_delta_cap = 0.05;

        let sub_coupon = evaluate_hedge_action(
            &config,
            &HedgeControllerState::default(),
            HedgeControllerInput {
                day: 2,
                spot: 0.85,
                spot_ratio: 0.85,
                note_notional: 1.0,
                raw_target_delta: 0.60,
                visible_state: VisibleState::NoCouponNoKnockInZone,
                observation_day: true,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 0,
                previous_spot_ratio: Some(0.90),
            },
        )
        .expect("sub coupon decision");
        assert!((sub_coupon.policy_target_delta - 0.20).abs() < 1e-12);

        let post_ki = evaluate_hedge_action(
            &config,
            &HedgeControllerState::default(),
            HedgeControllerInput {
                day: 2,
                spot: 0.72,
                spot_ratio: 0.72,
                note_notional: 1.0,
                raw_target_delta: 0.60,
                visible_state: VisibleState::NoCouponKnockInLatchedZone,
                observation_day: true,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 0,
                previous_spot_ratio: Some(0.75),
            },
        )
        .expect("post ki decision");
        assert!((post_ki.policy_target_delta - 0.05).abs() < 1e-12);
    }

    #[test]
    fn post_ki_zero_clears_inventory_after_latch() {
        let mut config = moderate_config();
        config.target_policy = HedgeTargetPolicy::PostKiZero;
        let decision = evaluate_hedge_action(
            &config,
            &HedgeControllerState {
                hedge_inventory_sol: 0.40,
                ..HedgeControllerState::default()
            },
            HedgeControllerInput {
                day: 8,
                spot: 0.74,
                spot_ratio: 0.74,
                note_notional: 1.0,
                raw_target_delta: 0.55,
                visible_state: VisibleState::NoCouponKnockInLatchedZone,
                observation_day: true,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 0,
                previous_spot_ratio: Some(0.73),
            },
        )
        .expect("decision");
        assert_eq!(decision.policy_target_delta, 0.0);
        assert!(decision.trade_quantity_sol < 0.0);
    }

    #[test]
    fn recovery_only_reenters_above_threshold() {
        let mut config = moderate_config();
        config.target_policy = HedgeTargetPolicy::RecoveryOnly;
        config.recovery_reentry_ratio = 0.92;
        config.recovery_reentry_delta_cap = 0.30;

        let blocked = evaluate_hedge_action(
            &config,
            &HedgeControllerState::default(),
            HedgeControllerInput {
                day: 10,
                spot: 0.85,
                spot_ratio: 0.85,
                note_notional: 1.0,
                raw_target_delta: 0.50,
                visible_state: VisibleState::NoCouponKnockInLatchedZone,
                observation_day: true,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 0,
                previous_spot_ratio: Some(0.84),
            },
        )
        .expect("blocked decision");
        assert_eq!(blocked.policy_target_delta, 0.0);

        let reentry = evaluate_hedge_action(
            &config,
            &HedgeControllerState::default(),
            HedgeControllerInput {
                day: 12,
                spot: 0.95,
                spot_ratio: 0.95,
                note_notional: 1.0,
                raw_target_delta: 0.50,
                visible_state: VisibleState::NoCouponKnockInLatchedZone,
                observation_day: true,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 0,
                previous_spot_ratio: Some(0.94),
            },
        )
        .expect("reentry decision");
        assert!((reentry.policy_target_delta - 0.30).abs() < 1e-12);
    }

    #[test]
    fn cost_aware_threshold_blocks_small_trade() {
        let mut config = moderate_config();
        config.target_policy = HedgeTargetPolicy::CostAware;
        config.cost_aware_expected_move = 0.01;
        config.cost_aware_threshold_multiple = 2.0;

        let decision = evaluate_hedge_action(
            &config,
            &HedgeControllerState {
                hedge_inventory_sol: 0.45,
                ..HedgeControllerState::default()
            },
            HedgeControllerInput {
                day: 6,
                spot: 1.0,
                spot_ratio: 1.0,
                note_notional: 1.0,
                raw_target_delta: 0.50,
                visible_state: VisibleState::CouponZone,
                observation_day: true,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 0,
                previous_spot_ratio: Some(0.99),
            },
        )
        .expect("decision");
        assert_eq!(decision.block_reason, HedgeBlockReason::CostThreshold);
        assert_eq!(decision.trade_quantity_sol, 0.0);
    }

    #[test]
    fn downside_ladder_caps_mid_deep_and_post_ki_regions() {
        let mut config = moderate_config();
        config.target_policy = HedgeTargetPolicy::DownsideLadder;
        config.downside_soft_threshold = 0.90;
        config.downside_deep_threshold = 0.80;
        config.downside_soft_cap = 0.50;
        config.downside_deep_cap = 0.30;
        config.post_ki_delta_cap = 0.10;

        let above_soft = evaluate_hedge_action(
            &config,
            &HedgeControllerState::default(),
            HedgeControllerInput {
                day: 4,
                spot: 0.95,
                spot_ratio: 0.95,
                note_notional: 1.0,
                raw_target_delta: 0.70,
                visible_state: VisibleState::NoCouponNoKnockInZone,
                observation_day: true,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 0,
                previous_spot_ratio: Some(0.96),
            },
        )
        .expect("above soft");
        assert!((above_soft.policy_target_delta - 0.70).abs() < 1e-12);

        let mid = evaluate_hedge_action(
            &config,
            &HedgeControllerState::default(),
            HedgeControllerInput {
                day: 4,
                spot: 0.85,
                spot_ratio: 0.85,
                note_notional: 1.0,
                raw_target_delta: 0.70,
                visible_state: VisibleState::NoCouponNoKnockInZone,
                observation_day: true,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 0,
                previous_spot_ratio: Some(0.86),
            },
        )
        .expect("mid");
        assert!((mid.policy_target_delta - 0.50).abs() < 1e-12);

        let deep = evaluate_hedge_action(
            &config,
            &HedgeControllerState::default(),
            HedgeControllerInput {
                day: 4,
                spot: 0.78,
                spot_ratio: 0.78,
                note_notional: 1.0,
                raw_target_delta: 0.70,
                visible_state: VisibleState::NoCouponNoKnockInZone,
                observation_day: true,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 0,
                previous_spot_ratio: Some(0.80),
            },
        )
        .expect("deep");
        assert!((deep.policy_target_delta - 0.30).abs() < 1e-12);

        let post_ki = evaluate_hedge_action(
            &config,
            &HedgeControllerState::default(),
            HedgeControllerInput {
                day: 4,
                spot: 0.68,
                spot_ratio: 0.68,
                note_notional: 1.0,
                raw_target_delta: 0.70,
                visible_state: VisibleState::KnockInTriggeredZone,
                observation_day: true,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 0,
                previous_spot_ratio: Some(0.71),
            },
        )
        .expect("post ki");
        assert!((post_ki.policy_target_delta - 0.10).abs() < 1e-12);
    }

    #[test]
    fn zone_aware_policy_lowers_coupon_zone_hedge_and_ramps_toward_ki() {
        let mut config = moderate_config();
        config.target_policy = HedgeTargetPolicy::ZoneAware;
        config.coupon_zone_floor_lambda = 0.15;
        config.near_ki_lambda = 0.85;

        let coupon_zone = evaluate_hedge_action(
            &config,
            &HedgeControllerState::default(),
            HedgeControllerInput {
                day: 5,
                spot: 1.02,
                spot_ratio: 1.02,
                note_notional: 1.0,
                raw_target_delta: 0.50,
                visible_state: VisibleState::CouponZone,
                observation_day: false,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 2,
                previous_spot_ratio: Some(1.01),
            },
        )
        .expect("coupon zone decision");
        assert!(coupon_zone.policy_target_delta < 0.10);

        let near_ki = evaluate_hedge_action(
            &config,
            &HedgeControllerState::default(),
            HedgeControllerInput {
                day: 5,
                spot: 0.74,
                spot_ratio: 0.74,
                note_notional: 1.0,
                raw_target_delta: 0.50,
                visible_state: VisibleState::NoCouponNoKnockInZone,
                observation_day: false,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 2,
                previous_spot_ratio: Some(0.76),
            },
        )
        .expect("near ki decision");
        assert!(near_ki.policy_target_delta > coupon_zone.policy_target_delta);
    }

    #[test]
    fn zone_aware_suppresses_upside_adds_near_observation() {
        let mut config = moderate_config();
        config.target_policy = HedgeTargetPolicy::ZoneAware;
        config.hedge_mode = HedgeMode::DeltaObservationPlusThreshold;
        config.coupon_zone_floor_lambda = 0.20;
        config.near_ki_lambda = 0.85;
        config.observation_hysteresis_days = 2;
        config.coupon_zone_band_multiplier = 3.0;

        let decision = evaluate_hedge_action(
            &config,
            &HedgeControllerState {
                hedge_inventory_sol: 0.20,
                ..HedgeControllerState::default()
            },
            HedgeControllerInput {
                day: 7,
                spot: 1.018,
                spot_ratio: 1.018,
                note_notional: 1.0,
                raw_target_delta: 0.60,
                visible_state: VisibleState::CouponZone,
                observation_day: false,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 1,
                previous_spot_ratio: Some(1.010),
            },
        )
        .expect("decision");
        assert_eq!(decision.executed_hedge_delta, 0.20);
        assert_eq!(decision.trade_quantity_sol, 0.0);
    }

    #[test]
    fn zone_aware_rebound_unwind_is_gradual() {
        let mut config = moderate_config();
        config.target_policy = HedgeTargetPolicy::ZoneAware;
        config.hedge_mode = HedgeMode::DeltaDaily;
        config.coupon_zone_floor_lambda = 0.15;
        config.near_ki_lambda = 0.85;
        config.post_ki_delta_cap = 0.20;
        config.rebound_unwind_half_life_days = 2.0;

        let decision = evaluate_hedge_action(
            &config,
            &HedgeControllerState {
                hedge_inventory_sol: 0.60,
                ..HedgeControllerState::default()
            },
            HedgeControllerInput {
                day: 9,
                spot: 0.93,
                spot_ratio: 0.93,
                note_notional: 1.0,
                raw_target_delta: 0.20,
                visible_state: VisibleState::NoCouponKnockInLatchedZone,
                observation_day: false,
                coupon_barrier: 1.0,
                autocall_barrier: 1.025,
                knock_in_barrier: 0.70,
                days_to_next_observation: 1,
                previous_spot_ratio: Some(0.88),
            },
        )
        .expect("decision");
        assert!(decision.executed_hedge_delta > 0.20);
        assert!(decision.executed_hedge_delta < 0.60);
    }
}
