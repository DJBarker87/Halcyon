//! Mid-life valuation for the SOL autocall.
//!
//! This module is deterministic and on-chain friendly. It reuses the same
//! fixed-grid Markov schedule surface as the SOL quote/backtest path, then
//! applies the same KI-capped collateral haircut convention used by the
//! flagship lending flow.

use serde::{Deserialize, Serialize};

use crate::autocall_v2::{
    build_midlife_transition_matrix_flat_s6, cu_trace,
    solve_markov_surface_pair_with_precomputed_matrices_s6,
    solve_markov_surface_pair_with_schedule_s6, AutocallParams, AutocallV2Error,
    MarkovScheduleStepS6, MarkovSurfacePair6, MarkovTransitionMatrixRef, MarkovValueSurface6,
    AUTOCALL_LOG_6, COS_M_MIDLIFE_COMPACT, KNOCK_IN_LOG_6,
};
use crate::generated::pod_deim_table::{
    TRAINING_ALPHA_S6, TRAINING_BETA_S6, TRAINING_REFERENCE_STEP_DAYS,
};

pub const SOL_AUTOCALL_OBSERVATION_COUNT: usize = 8;
pub const SCALE_S6: i64 = 1_000_000;
pub const BUYBACK_HAIRCUT_S6: i64 = 100_000;
pub const SOL_AUTOCALL_MIDLIFE_MARKOV_STATES: usize = 9;
pub const SOL_AUTOCALL_MIDLIFE_MATRIX_LEN: usize =
    SOL_AUTOCALL_MIDLIFE_MARKOV_STATES * SOL_AUTOCALL_MIDLIFE_MARKOV_STATES;
pub const SOL_AUTOCALL_MIDLIFE_COS_TERMS: u16 = COS_M_MIDLIFE_COMPACT as u16;

#[derive(Clone, Copy, Debug)]
pub struct SolAutocallMidlifeMatrixRef<'a> {
    pub step_days_s6: i64,
    pub values_s6: &'a [i64],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolAutocallMidlifeStatus {
    Active,
    AutoCalled,
    Settled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolAutocallMidlifeInputs {
    pub notional_usdc: u64,
    pub entry_price_s6: i64,
    pub current_price_s6: i64,
    pub autocall_barrier_s6: i64,
    pub coupon_barrier_s6: i64,
    pub ki_barrier_s6: i64,
    pub observation_schedule: [i64; SOL_AUTOCALL_OBSERVATION_COUNT],
    pub current_observation_index: u8,
    pub no_autocall_first_n_obs: u8,
    pub offered_coupon_bps_s6: i64,
    pub sigma_annual_s6: i64,
    pub ki_triggered: bool,
    pub status: SolAutocallMidlifeStatus,
    pub now_ts: i64,
    pub seconds_per_day: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolAutocallMidlifeNav {
    pub nav_s6: i64,
    pub ki_level_s6: i64,
    pub lending_value_s6: i64,
    pub nav_payout_usdc: u64,
    pub lending_value_payout_usdc: u64,
    pub remaining_coupon_pv_s6: i64,
    pub par_recovery_probability_s6: i64,
    pub due_coupon_count: u8,
    pub future_observation_count: u8,
    pub model_states: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolAutocallMidlifeError {
    InvalidInput,
    Overflow,
    InactivePolicy,
    Pricing,
}

pub fn price_midlife_nav(
    inputs: &SolAutocallMidlifeInputs,
) -> Result<SolAutocallMidlifeNav, SolAutocallMidlifeError> {
    cu_trace(b"cu_trace:sol_midlife:start");
    price_midlife_nav_with_states_and_matrices(inputs, SOL_AUTOCALL_MIDLIFE_MARKOV_STATES, None)
}

pub fn price_midlife_nav_with_matrices(
    inputs: &SolAutocallMidlifeInputs,
    matrices: &[SolAutocallMidlifeMatrixRef],
) -> Result<SolAutocallMidlifeNav, SolAutocallMidlifeError> {
    cu_trace(b"cu_trace:sol_midlife:start");
    price_midlife_nav_with_states_and_matrices(
        inputs,
        SOL_AUTOCALL_MIDLIFE_MARKOV_STATES,
        Some(matrices),
    )
}

fn price_midlife_nav_with_states(
    inputs: &SolAutocallMidlifeInputs,
    model_states: usize,
) -> Result<SolAutocallMidlifeNav, SolAutocallMidlifeError> {
    price_midlife_nav_with_states_and_matrices(inputs, model_states, None)
}

fn price_midlife_nav_with_states_and_matrices(
    inputs: &SolAutocallMidlifeInputs,
    model_states: usize,
    matrices: Option<&[SolAutocallMidlifeMatrixRef]>,
) -> Result<SolAutocallMidlifeNav, SolAutocallMidlifeError> {
    validate_inputs(inputs)?;
    if model_states < 7 {
        return Err(SolAutocallMidlifeError::InvalidInput);
    }
    if inputs.status != SolAutocallMidlifeStatus::Active {
        return Err(SolAutocallMidlifeError::InactivePolicy);
    }

    let ki_level_s6 = ratio_s6(inputs.ki_barrier_s6, inputs.entry_price_s6)?;
    let current_ratio_s6 = ratio_s6(inputs.current_price_s6.max(0), inputs.entry_price_s6)?;

    let due = due_observation_state(inputs);
    let coupon_fraction_s6 = coupon_bps_s6_to_fraction_s6(inputs.offered_coupon_bps_s6)?;
    let due_coupon_pv_s6 = mul_i64_i64(coupon_fraction_s6, i64::from(due.due_coupon_count))?;

    let (future_nav_s6, future_redemption_s6, future_observation_count) =
        if let Some(terminal_principal_s6) = due.terminal_principal_s6 {
            (terminal_principal_s6, terminal_principal_s6, 0)
        } else {
            let schedule = future_schedule(inputs, due.next_observation_index)?;
            if schedule.is_empty() {
                return Err(SolAutocallMidlifeError::InvalidInput);
            }
            cu_trace(b"cu_trace:sol_midlife:after_schedule");
            let contract = AutocallParams {
                n_obs: SOL_AUTOCALL_OBSERVATION_COUNT,
                knock_in_log_6: KNOCK_IN_LOG_6,
                autocall_log_6: AUTOCALL_LOG_6,
                no_autocall_first_n_obs: inputs.no_autocall_first_n_obs as usize,
            };
            let surfaces = price_surfaces(
                inputs.sigma_annual_s6,
                &contract,
                &schedule,
                coupon_fraction_s6,
                model_states,
                matrices,
            )?;
            cu_trace(b"cu_trace:sol_midlife:after_surfaces");
            let nav = interpolate_surface_value(&surfaces.nav, current_ratio_s6, due.ki_triggered)?;
            let redemption = interpolate_surface_value(
                &surfaces.redemption,
                current_ratio_s6,
                due.ki_triggered,
            )?;
            (
                nav.max(0),
                redemption.clamp(0, SCALE_S6),
                schedule.iter().filter(|step| step.observation).count() as u8 + 1,
            )
        };

    let nav_s6 = future_nav_s6
        .checked_add(due_coupon_pv_s6)
        .ok_or(SolAutocallMidlifeError::Overflow)?
        .max(0);
    let future_coupon_pv_s6 = future_nav_s6.saturating_sub(future_redemption_s6).max(0);
    let remaining_coupon_pv_s6 = due_coupon_pv_s6
        .checked_add(future_coupon_pv_s6)
        .ok_or(SolAutocallMidlifeError::Overflow)?;
    let par_recovery_probability_s6 = future_redemption_s6.clamp(0, SCALE_S6);
    let lending_value_s6 = discounted_value_s6(nav_s6, ki_level_s6, BUYBACK_HAIRCUT_S6);

    Ok(SolAutocallMidlifeNav {
        nav_s6,
        ki_level_s6,
        lending_value_s6,
        nav_payout_usdc: amount_mul_s6(inputs.notional_usdc, nav_s6)?,
        lending_value_payout_usdc: amount_mul_s6(inputs.notional_usdc, lending_value_s6)?,
        remaining_coupon_pv_s6,
        par_recovery_probability_s6,
        due_coupon_count: due.due_coupon_count,
        future_observation_count,
        model_states: model_states as u16,
    })
}

pub fn discounted_value_s6(nav_s6: i64, ki_level_s6: i64, haircut_s6: i64) -> i64 {
    let nav_less_haircut = nav_s6.saturating_sub(haircut_s6);
    let ki_less_haircut = ki_level_s6.saturating_sub(haircut_s6);
    nav_less_haircut.min(ki_less_haircut).max(0)
}

fn validate_inputs(inputs: &SolAutocallMidlifeInputs) -> Result<(), SolAutocallMidlifeError> {
    if inputs.notional_usdc == 0
        || inputs.entry_price_s6 <= 0
        || inputs.current_price_s6 <= 0
        || inputs.autocall_barrier_s6 <= 0
        || inputs.coupon_barrier_s6 <= 0
        || inputs.ki_barrier_s6 <= 0
        || inputs.current_observation_index as usize > SOL_AUTOCALL_OBSERVATION_COUNT
        || inputs.offered_coupon_bps_s6 < 0
        || inputs.sigma_annual_s6 <= 0
        || inputs.seconds_per_day <= 0
    {
        return Err(SolAutocallMidlifeError::InvalidInput);
    }
    Ok(())
}

struct DueObservationState {
    due_coupon_count: u8,
    next_observation_index: usize,
    ki_triggered: bool,
    terminal_principal_s6: Option<i64>,
}

fn due_observation_state(inputs: &SolAutocallMidlifeInputs) -> DueObservationState {
    let mut due_coupon_count = 0u8;
    let mut terminal_principal_s6 = None;
    let mut ki_triggered = inputs.ki_triggered;
    let mut idx = inputs.current_observation_index as usize;
    while idx < SOL_AUTOCALL_OBSERVATION_COUNT && inputs.now_ts >= inputs.observation_schedule[idx]
    {
        if inputs.current_price_s6 >= inputs.coupon_barrier_s6 {
            due_coupon_count = due_coupon_count.saturating_add(1);
        }
        if inputs.current_price_s6 <= inputs.ki_barrier_s6 {
            ki_triggered = true;
        }
        let observation_index = idx as u8;
        let autocall_allowed = observation_index >= inputs.no_autocall_first_n_obs;
        if autocall_allowed && inputs.current_price_s6 >= inputs.autocall_barrier_s6 {
            terminal_principal_s6 = Some(SCALE_S6);
            break;
        }
        if idx + 1 == SOL_AUTOCALL_OBSERVATION_COUNT {
            let current_ratio_s6 =
                ratio_s6(inputs.current_price_s6.max(0), inputs.entry_price_s6).unwrap_or(0);
            let principal_s6 = if ki_triggered && inputs.current_price_s6 < inputs.entry_price_s6 {
                current_ratio_s6.min(SCALE_S6).max(0)
            } else {
                SCALE_S6
            };
            terminal_principal_s6 = Some(principal_s6);
            break;
        }
        idx += 1;
    }

    DueObservationState {
        due_coupon_count,
        next_observation_index: if terminal_principal_s6.is_some() {
            idx.saturating_add(1)
        } else {
            idx
        },
        ki_triggered,
        terminal_principal_s6,
    }
}

fn coupon_bps_s6_to_fraction_s6(coupon_bps_s6: i64) -> Result<i64, SolAutocallMidlifeError> {
    if coupon_bps_s6 < 0 {
        return Err(SolAutocallMidlifeError::InvalidInput);
    }
    Ok(coupon_bps_s6 / 10_000)
}

fn future_schedule(
    inputs: &SolAutocallMidlifeInputs,
    start_observation_index: usize,
) -> Result<Vec<MarkovScheduleStepS6>, SolAutocallMidlifeError> {
    if start_observation_index >= SOL_AUTOCALL_OBSERVATION_COUNT {
        return Ok(Vec::new());
    }
    let mut cursor_ts = inputs.now_ts;
    let mut schedule = Vec::with_capacity(SOL_AUTOCALL_OBSERVATION_COUNT - start_observation_index);
    for (position, obs_idx) in (start_observation_index..SOL_AUTOCALL_OBSERVATION_COUNT).enumerate()
    {
        let obs_ts = inputs.observation_schedule[obs_idx];
        if obs_ts <= cursor_ts {
            return Err(SolAutocallMidlifeError::InvalidInput);
        }
        schedule.push(MarkovScheduleStepS6 {
            step_days_s6: seconds_to_days_s6(obs_ts - cursor_ts, inputs.seconds_per_day)?,
            observation: position != 0,
            obs_index_from_inception: if position == 0 { 0 } else { obs_idx },
        });
        cursor_ts = obs_ts;
    }
    Ok(schedule)
}

fn seconds_to_days_s6(seconds: i64, seconds_per_day: i64) -> Result<i64, SolAutocallMidlifeError> {
    if seconds <= 0 || seconds_per_day <= 0 {
        return Err(SolAutocallMidlifeError::InvalidInput);
    }
    let scaled = i128::from(seconds)
        .checked_mul(i128::from(SCALE_S6))
        .and_then(|value| value.checked_div(i128::from(seconds_per_day)))
        .ok_or(SolAutocallMidlifeError::Overflow)?;
    i64::try_from(scaled.max(1)).map_err(|_| SolAutocallMidlifeError::Overflow)
}

fn price_surfaces(
    sigma_annual_s6: i64,
    contract: &AutocallParams,
    schedule: &[MarkovScheduleStepS6],
    coupon_s6: i64,
    model_states: usize,
    matrices: Option<&[SolAutocallMidlifeMatrixRef]>,
) -> Result<MarkovSurfacePair6, SolAutocallMidlifeError> {
    if let Some(matrices) = matrices {
        let matrix_refs = matrices
            .iter()
            .map(|matrix| MarkovTransitionMatrixRef {
                step_days_s6: matrix.step_days_s6,
                values_s6: matrix.values_s6,
            })
            .collect::<Vec<_>>();
        solve_markov_surface_pair_with_precomputed_matrices_s6(
            sigma_annual_s6,
            TRAINING_ALPHA_S6,
            TRAINING_BETA_S6,
            TRAINING_REFERENCE_STEP_DAYS,
            model_states,
            contract,
            schedule,
            coupon_s6,
            &matrix_refs,
        )
        .map_err(map_pricing_error)
    } else {
        solve_markov_surface_pair_with_schedule_s6(
            sigma_annual_s6,
            TRAINING_ALPHA_S6,
            TRAINING_BETA_S6,
            TRAINING_REFERENCE_STEP_DAYS,
            model_states,
            contract,
            schedule,
            coupon_s6,
        )
        .map_err(map_pricing_error)
    }
}

pub fn build_midlife_transition_matrix_for_upload(
    sigma_annual_s6: i64,
    step_days_s6: i64,
) -> Result<Vec<i64>, SolAutocallMidlifeError> {
    let contract = AutocallParams {
        n_obs: SOL_AUTOCALL_OBSERVATION_COUNT,
        knock_in_log_6: KNOCK_IN_LOG_6,
        autocall_log_6: AUTOCALL_LOG_6,
        no_autocall_first_n_obs: 1,
    };
    build_midlife_transition_matrix_flat_s6(
        sigma_annual_s6,
        TRAINING_ALPHA_S6,
        TRAINING_BETA_S6,
        TRAINING_REFERENCE_STEP_DAYS,
        SOL_AUTOCALL_MIDLIFE_MARKOV_STATES,
        &contract,
        step_days_s6,
    )
    .map_err(map_pricing_error)
}

fn map_pricing_error(_err: AutocallV2Error) -> SolAutocallMidlifeError {
    SolAutocallMidlifeError::Pricing
}

fn interpolate_surface_value(
    surface: &MarkovValueSurface6,
    spot_ratio_s6: i64,
    ki_triggered: bool,
) -> Result<i64, SolAutocallMidlifeError> {
    let values = if ki_triggered {
        &surface.touched_values_6
    } else {
        &surface.untouched_values_6
    };
    interpolate_s6(&surface.spot_ratios_6, values, spot_ratio_s6)
}

fn interpolate_s6(xs: &[i64], ys: &[i64], x: i64) -> Result<i64, SolAutocallMidlifeError> {
    if xs.is_empty() || xs.len() != ys.len() {
        return Err(SolAutocallMidlifeError::InvalidInput);
    }
    if x <= xs[0] {
        return Ok(ys[0]);
    }
    let last = xs.len() - 1;
    if x >= xs[last] {
        return Ok(ys[last]);
    }
    let upper = match xs.binary_search(&x) {
        Ok(idx) => return Ok(ys[idx]),
        Err(idx) => idx,
    };
    let lower = upper.saturating_sub(1);
    let width = xs[upper] - xs[lower];
    if width <= 0 {
        return Err(SolAutocallMidlifeError::InvalidInput);
    }
    let dy = ys[upper] - ys[lower];
    let dx = x - xs[lower];
    let value = i128::from(ys[lower])
        .checked_add(
            i128::from(dy)
                .checked_mul(i128::from(dx))
                .and_then(|value| value.checked_div(i128::from(width)))
                .ok_or(SolAutocallMidlifeError::Overflow)?,
        )
        .ok_or(SolAutocallMidlifeError::Overflow)?;
    i64::try_from(value).map_err(|_| SolAutocallMidlifeError::Overflow)
}

fn ratio_s6(numerator_s6: i64, denominator_s6: i64) -> Result<i64, SolAutocallMidlifeError> {
    if denominator_s6 <= 0 || numerator_s6 < 0 {
        return Err(SolAutocallMidlifeError::InvalidInput);
    }
    let scaled = i128::from(numerator_s6)
        .checked_mul(i128::from(SCALE_S6))
        .and_then(|value| value.checked_div(i128::from(denominator_s6)))
        .ok_or(SolAutocallMidlifeError::Overflow)?;
    i64::try_from(scaled).map_err(|_| SolAutocallMidlifeError::Overflow)
}

fn mul_i64_i64(lhs: i64, rhs: i64) -> Result<i64, SolAutocallMidlifeError> {
    lhs.checked_mul(rhs)
        .ok_or(SolAutocallMidlifeError::Overflow)
}

fn amount_mul_s6(amount: u64, fraction_s6: i64) -> Result<u64, SolAutocallMidlifeError> {
    if fraction_s6 <= 0 {
        return Ok(0);
    }
    let value = (amount as u128)
        .checked_mul(fraction_s6 as u128)
        .and_then(|value| value.checked_div(SCALE_S6 as u128))
        .ok_or(SolAutocallMidlifeError::Overflow)?;
    u64::try_from(value).map_err(|_| SolAutocallMidlifeError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_inputs() -> SolAutocallMidlifeInputs {
        SolAutocallMidlifeInputs {
            notional_usdc: 10_000_000_000,
            entry_price_s6: 100_000_000,
            current_price_s6: 105_000_000,
            autocall_barrier_s6: 102_500_000,
            coupon_barrier_s6: 100_000_000,
            ki_barrier_s6: 70_000_000,
            observation_schedule: [10, 20, 30, 40, 50, 60, 70, 80],
            current_observation_index: 0,
            no_autocall_first_n_obs: 1,
            offered_coupon_bps_s6: 200_000_000,
            sigma_annual_s6: 500_000,
            ki_triggered: false,
            status: SolAutocallMidlifeStatus::Active,
            now_ts: 1,
            seconds_per_day: 10,
        }
    }

    #[test]
    fn healthy_note_lending_value_is_ki_capped() {
        let nav = price_midlife_nav(&sample_inputs()).expect("nav");
        assert!(nav.nav_s6 > SCALE_S6);
        assert_eq!(nav.ki_level_s6, 700_000);
        assert_eq!(nav.lending_value_s6, 600_000);
        assert_eq!(nav.lending_value_payout_usdc, 6_000_000_000);
        assert_eq!(nav.model_states, SOL_AUTOCALL_MIDLIFE_MARKOV_STATES as u16);
    }

    #[test]
    fn knocked_note_tracks_live_recovery() {
        let mut inputs = sample_inputs();
        inputs.current_price_s6 = 50_000_000;
        inputs.ki_triggered = true;
        inputs.now_ts = 80;
        let nav = price_midlife_nav(&inputs).expect("nav");
        assert_eq!(nav.nav_s6, 500_000);
        assert_eq!(nav.par_recovery_probability_s6, 500_000);
        assert_eq!(nav.lending_value_s6, 400_000);
    }

    #[test]
    fn due_coupon_is_included_before_keeper_records_it() {
        let mut inputs = sample_inputs();
        inputs.now_ts = 10;
        let nav = price_midlife_nav(&inputs).expect("nav");
        assert_eq!(nav.due_coupon_count, 1);
        assert!(nav.remaining_coupon_pv_s6 >= 20_000);
    }

    #[test]
    fn future_value_declines_after_coupon_miss_backtest() {
        let mut above_coupon = sample_inputs();
        above_coupon.current_price_s6 = 100_000_000;
        above_coupon.now_ts = 11;
        let above = price_midlife_nav(&above_coupon).expect("above");

        let mut below_coupon = above_coupon;
        below_coupon.current_price_s6 = 95_000_000;
        let below = price_midlife_nav(&below_coupon).expect("below");

        assert!(above.nav_s6 > below.nav_s6);
        assert!(above.remaining_coupon_pv_s6 > below.remaining_coupon_pv_s6);
        assert!(below.lending_value_s6 <= below.nav_s6);
    }

    #[test]
    fn no_overstated_lending_value_across_price_backtest_grid() {
        let mut inputs = sample_inputs();
        for price in [
            45_000_000,
            60_000_000,
            70_000_000,
            80_000_000,
            95_000_000,
            100_000_000,
            110_000_000,
            140_000_000,
        ] {
            inputs.current_price_s6 = price;
            let nav = price_midlife_nav(&inputs).expect("nav");
            assert!(nav.lending_value_s6 <= nav.nav_s6);
            assert!(nav.lending_value_s6 <= nav.ki_level_s6 - BUYBACK_HAIRCUT_S6);
            assert!(nav.par_recovery_probability_s6 >= 0);
            assert!(nav.par_recovery_probability_s6 <= SCALE_S6);
        }
    }

    #[test]
    fn production_state_grid_tracks_higher_state_reference_backtest() {
        let mut inputs = sample_inputs();
        let now_grid = [1, 5, 9, 11, 15, 19, 21, 31, 41, 59];
        let price_grid = [
            55_000_000,
            65_000_000,
            70_000_000,
            75_000_000,
            85_000_000,
            95_000_000,
            100_000_000,
            105_000_000,
            120_000_000,
        ];
        for ki_triggered in [false, true] {
            inputs.ki_triggered = ki_triggered;
            for now_ts in now_grid {
                for current_price_s6 in price_grid {
                    if !ki_triggered && current_price_s6 <= inputs.ki_barrier_s6 {
                        continue;
                    }
                    inputs.now_ts = now_ts;
                    inputs.current_price_s6 = current_price_s6;
                    let production =
                        price_midlife_nav_with_states(&inputs, SOL_AUTOCALL_MIDLIFE_MARKOV_STATES)
                            .expect("production nav");
                    let reference =
                        price_midlife_nav_with_states(&inputs, 21).expect("reference nav");
                    let nav_error = (production.nav_s6 - reference.nav_s6).abs();
                    let redemption_error = (production.par_recovery_probability_s6
                        - reference.par_recovery_probability_s6)
                        .abs();
                    assert!(
                        nav_error <= 35_000,
                        "nav error too high at now={} price={}: production={} reference={} error={}",
                        now_ts,
                        current_price_s6,
                        production.nav_s6,
                        reference.nav_s6,
                        nav_error
                    );
                    assert!(
                        redemption_error <= 35_000,
                        "redemption error too high at now={} price={}: production={} reference={} error={}",
                        now_ts,
                        current_price_s6,
                        production.par_recovery_probability_s6,
                        reference.par_recovery_probability_s6,
                        redemption_error
                    );
                    assert!(production.lending_value_s6 <= production.nav_s6);
                }
            }
        }
    }
}
