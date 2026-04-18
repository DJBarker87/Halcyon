//! Thin adapter from the hedged replay stack into the current `autocall_v2`
//! live-quote path.
//!
//! Reused from `autocall_v2.rs`:
//! - contract barrier semantics
//! - terminal redemption and accrued-coupon conventions
//! - cached E11 live-operator quote inside the validated sigma band
//! - gated Markov/Richardson fallback outside that band
//! - NIG transition construction and dense Zhang-Li schedule surfaces for replay deltas
//!
//! Wrapped here:
//! - mapping from research/autocall replay terms into `autocall_v2`
//! - day-specific parity surfaces for replay-time value/delta lookups
//! - a fixed-grid intraperiod extension for the 1-day stub between observation dates
//!   so the hedge controller can rebalance on the daily replay grid without creating a
//!   separate pricing universe

use crate::autocall_hedged::{
    AutocallTerms, CouponQuoteMode, DailySurface, HedgedAutocallError, PricingModel,
    PricingOutputs, RuntimeDiagnostics,
};
use crate::autocall_v2::{
    solve_fair_coupon_markov_richardson_gated, solve_markov_surface_with_schedule, AutocallParams,
    MarkovScheduleStep, NigParams6,
};
use crate::autocall_v2_e11::{live_quote_uses_e11, solve_fair_coupon_e11_cached};
use solmath_core::{SCALE, SCALE_6};

pub const PARITY_N1: usize = 10;
pub const PARITY_N2: usize = 15;

#[derive(Debug, Clone, PartialEq)]
pub struct ParityPricedAutocall {
    pub pricing: PricingOutputs,
    pub observation_days: Vec<usize>,
    pub surfaces: Vec<DailySurface>,
    pub assumptions: Vec<String>,
}

#[derive(Debug, Clone)]
struct RichardsonSurface {
    spot_ratios: Vec<f64>,
    untouched_values: Vec<f64>,
    touched_values: Vec<f64>,
    untouched_deltas: Vec<f64>,
    touched_deltas: Vec<f64>,
}

fn to_scale6(value: f64) -> Result<i64, HedgedAutocallError> {
    if !value.is_finite() {
        return Err(HedgedAutocallError::InvalidModel("value must be finite"));
    }
    Ok((value * SCALE_6 as f64).round() as i64)
}

fn from_scale6(value: i64) -> f64 {
    value as f64 / SCALE_6 as f64
}

fn richardson_scalar(n1: usize, n2: usize, low: f64, high: f64) -> f64 {
    let n1_sq = (n1 * n1) as f64;
    let n2_sq = (n2 * n2) as f64;
    (n2_sq * high - n1_sq * low) / (n2_sq - n1_sq)
}

fn schedule_observation_days(terms: &AutocallTerms) -> Vec<usize> {
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

fn build_contract(terms: &AutocallTerms) -> Result<AutocallParams, HedgedAutocallError> {
    Ok(AutocallParams {
        n_obs: schedule_observation_days(terms).len(),
        knock_in_log_6: to_scale6(terms.knock_in_barrier.ln())?,
        autocall_log_6: to_scale6(terms.autocall_barrier.ln())?,
        no_autocall_first_n_obs: terms.no_autocall_first_n_obs,
    })
}

fn schedule_for_day(terms: &AutocallTerms, day: usize) -> Vec<MarkovScheduleStep> {
    let all_observation_days = schedule_observation_days(terms);
    let future_observations = all_observation_days
        .iter()
        .copied()
        .filter(|obs_day| *obs_day > day)
        .collect::<Vec<_>>();

    let mut cursor = day;
    let mut schedule = Vec::with_capacity(future_observations.len());
    for obs_day in &future_observations {
        schedule.push(MarkovScheduleStep {
            step_days: (*obs_day - cursor) as i64,
            observation: cursor != 0 || day != 0 && day == cursor,
            obs_index_from_inception: 0, // filled below
        });
        cursor = *obs_day;
    }
    if let Some(first) = schedule.first_mut() {
        first.observation = day != 0 && all_observation_days.iter().any(|obs_day| *obs_day == day);
    }
    // Assign observation indices: the first observation at/after the cursor's
    // starting day maps to its position in the full observation schedule (1-indexed).
    let is_on_obs = all_observation_days.contains(&day);
    let mut obs_counter = if is_on_obs {
        all_observation_days.iter().position(|&d| d == day).unwrap() + 1
    } else {
        all_observation_days
            .iter()
            .position(|&d| d > day)
            .unwrap_or(all_observation_days.len())
    };
    for step in &mut schedule {
        if step.observation {
            step.obs_index_from_inception = obs_counter;
        }
        // Next observation in the full schedule
        obs_counter += 1;
    }
    schedule
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

fn merge_surface_pair(
    low_surface: &crate::autocall_v2::MarkovSurface6,
    high_surface: &crate::autocall_v2::MarkovSurface6,
) -> RichardsonSurface {
    let mut merged_spots = low_surface
        .spot_ratios_6
        .iter()
        .chain(high_surface.spot_ratios_6.iter())
        .copied()
        .collect::<Vec<_>>();
    merged_spots.sort_unstable();
    merged_spots.dedup();

    let low_spots = low_surface
        .spot_ratios_6
        .iter()
        .map(|value| from_scale6(*value))
        .collect::<Vec<_>>();
    let high_spots = high_surface
        .spot_ratios_6
        .iter()
        .map(|value| from_scale6(*value))
        .collect::<Vec<_>>();
    let merged_spots_f64 = merged_spots
        .iter()
        .map(|value| from_scale6(*value))
        .collect::<Vec<_>>();

    let low_untouched = low_surface
        .untouched_values_6
        .iter()
        .map(|value| from_scale6(*value))
        .collect::<Vec<_>>();
    let high_untouched = high_surface
        .untouched_values_6
        .iter()
        .map(|value| from_scale6(*value))
        .collect::<Vec<_>>();
    let low_touched = low_surface
        .touched_values_6
        .iter()
        .map(|value| from_scale6(*value))
        .collect::<Vec<_>>();
    let high_touched = high_surface
        .touched_values_6
        .iter()
        .map(|value| from_scale6(*value))
        .collect::<Vec<_>>();
    let low_untouched_deltas = low_surface
        .untouched_deltas_6
        .iter()
        .map(|value| from_scale6(*value))
        .collect::<Vec<_>>();
    let high_untouched_deltas = high_surface
        .untouched_deltas_6
        .iter()
        .map(|value| from_scale6(*value))
        .collect::<Vec<_>>();
    let low_touched_deltas = low_surface
        .touched_deltas_6
        .iter()
        .map(|value| from_scale6(*value))
        .collect::<Vec<_>>();
    let high_touched_deltas = high_surface
        .touched_deltas_6
        .iter()
        .map(|value| from_scale6(*value))
        .collect::<Vec<_>>();

    let untouched_values = merged_spots_f64
        .iter()
        .map(|spot| {
            richardson_scalar(
                PARITY_N1,
                PARITY_N2,
                interpolate(&low_spots, &low_untouched, *spot),
                interpolate(&high_spots, &high_untouched, *spot),
            )
        })
        .collect::<Vec<_>>();
    let touched_values = merged_spots_f64
        .iter()
        .map(|spot| {
            richardson_scalar(
                PARITY_N1,
                PARITY_N2,
                interpolate(&low_spots, &low_touched, *spot),
                interpolate(&high_spots, &high_touched, *spot),
            )
        })
        .collect::<Vec<_>>();
    let untouched_deltas = merged_spots_f64
        .iter()
        .map(|spot| {
            richardson_scalar(
                PARITY_N1,
                PARITY_N2,
                interpolate(&low_spots, &low_untouched_deltas, *spot),
                interpolate(&high_spots, &high_untouched_deltas, *spot),
            )
        })
        .collect::<Vec<_>>();
    let touched_deltas = merged_spots_f64
        .iter()
        .map(|spot| {
            richardson_scalar(
                PARITY_N1,
                PARITY_N2,
                interpolate(&low_spots, &low_touched_deltas, *spot),
                interpolate(&high_spots, &high_touched_deltas, *spot),
            )
        })
        .collect::<Vec<_>>();

    RichardsonSurface {
        spot_ratios: merged_spots_f64,
        untouched_values,
        touched_values,
        untouched_deltas,
        touched_deltas,
    }
}

fn schedule_surface(
    sigma_ann_6: i64,
    alpha_6: i64,
    beta_6: i64,
    reference_step_days: i64,
    contract: &AutocallParams,
    schedule: &[MarkovScheduleStep],
    coupon_6: i64,
) -> Result<
    (
        crate::autocall_v2::MarkovSurface6,
        crate::autocall_v2::MarkovSurface6,
    ),
    HedgedAutocallError,
> {
    let low = solve_markov_surface_with_schedule(
        sigma_ann_6,
        alpha_6,
        beta_6,
        reference_step_days,
        PARITY_N1,
        contract,
        schedule,
        coupon_6,
    )
    .map_err(|error| {
        HedgedAutocallError::Numerical(Box::leak(format!("{error:?}").into_boxed_str()))
    })?;
    let high = solve_markov_surface_with_schedule(
        sigma_ann_6,
        alpha_6,
        beta_6,
        reference_step_days,
        PARITY_N2,
        contract,
        schedule,
        coupon_6,
    )
    .map_err(|error| {
        HedgedAutocallError::Numerical(Box::leak(format!("{error:?}").into_boxed_str()))
    })?;
    Ok((low, high))
}

fn map_v2_error(error: impl std::fmt::Debug) -> HedgedAutocallError {
    HedgedAutocallError::Numerical(Box::leak(format!("{error:?}").into_boxed_str()))
}

pub fn price_autocall_v2_parity(
    terms: &AutocallTerms,
    model: &PricingModel,
) -> Result<ParityPricedAutocall, HedgedAutocallError> {
    let contract = build_contract(terms)?;
    let observation_days = schedule_observation_days(terms);
    let reference_step_days = terms.observation_interval_days as i64;
    let sigma_ann_6 = to_scale6(model.sigma_ann)?;
    let alpha_6 = to_scale6(model.alpha)?;
    let beta_6 = to_scale6(model.beta)?;
    let nig =
        NigParams6::from_vol_with_step_days(sigma_ann_6, alpha_6, beta_6, reference_step_days)
            .map_err(map_v2_error)?;
    let (direct_quote, live_quote_engine) = if live_quote_uses_e11(model.sigma_ann, &contract) {
        (
            solve_fair_coupon_e11_cached(
                sigma_ann_6,
                alpha_6,
                beta_6,
                reference_step_days,
                &contract,
            )
            .map_err(map_v2_error)?,
            "e11_live_operator_quote",
        )
    } else {
        (
            solve_fair_coupon_markov_richardson_gated(&nig, PARITY_N1, PARITY_N2, &contract)
                .map_err(map_v2_error)?
                .result,
            "gated_markov_richardson_fallback",
        )
    };
    let gated_cross_check =
        solve_fair_coupon_markov_richardson_gated(&nig, PARITY_N1, PARITY_N2, &contract)
            .map_err(map_v2_error)?;
    let direct_fair_coupon = direct_quote.fair_coupon as f64 / SCALE as f64;
    let direct_expected_redemption = direct_quote.expected_redemption as f64 / SCALE as f64;
    let direct_expected_coupon_count = direct_quote.expected_coupon_count as f64 / SCALE as f64;
    let gated_fair_coupon = gated_cross_check.result.fair_coupon as f64 / SCALE as f64;

    let day0_schedule = schedule_for_day(terms, 0);
    let (zero_low, zero_high) = schedule_surface(
        sigma_ann_6,
        alpha_6,
        beta_6,
        reference_step_days,
        &contract,
        &day0_schedule,
        0,
    )?;
    let (unit_low, unit_high) = schedule_surface(
        sigma_ann_6,
        alpha_6,
        beta_6,
        reference_step_days,
        &contract,
        &day0_schedule,
        SCALE_6,
    )?;
    let expected_redemption = richardson_scalar(
        PARITY_N1,
        PARITY_N2,
        from_scale6(zero_low.untouched_values_6[zero_low.atm_state]),
        from_scale6(zero_high.untouched_values_6[zero_high.atm_state]),
    )
    .max(0.0);
    let expected_coupon_count = (richardson_scalar(
        PARITY_N1,
        PARITY_N2,
        from_scale6(unit_low.untouched_values_6[unit_low.atm_state]),
        from_scale6(unit_high.untouched_values_6[unit_high.atm_state]),
    ) - expected_redemption)
        .max(0.0);
    let day0_surface_redemption = expected_redemption;
    let day0_surface_coupon_count = expected_coupon_count;
    let fair_coupon = direct_fair_coupon;
    let expected_redemption = direct_expected_redemption;
    let expected_coupon_count = direct_expected_coupon_count;
    let quoted_coupon = match terms.coupon_quote_mode {
        CouponQuoteMode::ShareOfFair => fair_coupon * terms.quote_share_of_fair_coupon,
        CouponQuoteMode::FixedPerObservation(value) => value,
    };
    let explicit_margin = terms.notional * terms.issuer_margin_bps / 10_000.0;
    let fair_edge = (fair_coupon - quoted_coupon) * expected_coupon_count + explicit_margin;
    let quoted_coupon_6 = to_scale6(quoted_coupon)?;

    let mut surfaces = Vec::with_capacity(terms.maturity_days + 1);
    let mut expected_liability_profile = Vec::with_capacity(terms.maturity_days + 1);
    for day in 0..=terms.maturity_days {
        let schedule = schedule_for_day(terms, day);
        let (low_surface, high_surface) = schedule_surface(
            sigma_ann_6,
            alpha_6,
            beta_6,
            reference_step_days,
            &contract,
            &schedule,
            quoted_coupon_6,
        )?;
        let merged = merge_surface_pair(&low_surface, &high_surface);
        expected_liability_profile.push(interpolate(
            &merged.spot_ratios,
            &merged.untouched_values,
            1.0,
        ));
        surfaces.push(DailySurface {
            day,
            spot_ratios: merged.spot_ratios,
            untouched_values: merged.untouched_values,
            touched_values: merged.touched_values,
            untouched_deltas: merged.untouched_deltas,
            touched_deltas: merged.touched_deltas,
        });
    }

    let expected_payout = expected_redemption + quoted_coupon * expected_coupon_count;
    if let Some(day0) = expected_liability_profile.first_mut() {
        *day0 = expected_payout;
    }
    let max_liability = expected_liability_profile
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let reserve_need = (max_liability - terms.notional).max(0.0);
    let expected_coupon_stream = quoted_coupon * expected_coupon_count;
    let atm_delta = interpolate(&surfaces[0].spot_ratios, &surfaces[0].untouched_deltas, 1.0);

    let assumptions = vec![
        "Parity uses the live autocall_v2 issuance quote path first, then keeps the current replay settlement semantics unchanged.".to_string(),
        "Issuance-time fair coupon and expected decomposition use cached E11 live-operator pricing for the validated 8-observation current-v1 note shape inside the 50%-250% sigma band, with gated Markov/Richardson fallback outside that range or on unsupported contract shapes.".to_string(),
        "Intraperiod hedge checks still reuse dense autocall_v2 Zhang-Li schedule surfaces with a shorter first-step transition into the next observation because the repo does not yet expose a reusable E11 day-surface adapter.".to_string(),
        "Candidate reruns replay every overlapping historical window from the local SOL hourly dataset and do not re-run the earlier coarse issuance frontier.".to_string(),
        format!(
            "Day-0 quote engine: {}. adapter={:.6}, gated_cross_check={:.6}, diff_bps={:.2}",
            live_quote_engine,
            fair_coupon,
            gated_fair_coupon,
            (fair_coupon - gated_fair_coupon) * 10_000.0
        ),
        format!(
            "Day-0 surface cross-check: redemption_surface={:.6}, redemption_direct={:.6}, coupon_count_surface={:.6}, coupon_count_direct={:.6}",
            day0_surface_redemption,
            direct_expected_redemption,
            day0_surface_coupon_count,
            direct_expected_coupon_count
        ),
    ];

    Ok(ParityPricedAutocall {
        pricing: PricingOutputs {
            fair_coupon_per_observation: fair_coupon,
            quoted_coupon_per_observation: quoted_coupon,
            expected_payout,
            expected_coupon_stream,
            expected_liability_profile,
            max_liability,
            reserve_need,
            expected_coupon_count,
            expected_redemption,
            fair_edge,
            explicit_margin,
            atm_delta,
            diagnostics: RuntimeDiagnostics {
                grid_points: PARITY_N2,
                log_step: 0.0,
                day_steps: terms.maturity_days,
                observation_days: observation_days.clone(),
                kernel_half_width: 0,
                kernel_mass: 1.0,
                convolution_ops: 0,
                bridge_ops: 0,
                delta_ops: surfaces
                    .iter()
                    .map(|surface| surface.untouched_deltas.len() + surface.touched_deltas.len())
                    .sum(),
            },
        },
        observation_days,
        surfaces,
        assumptions,
    })
}
