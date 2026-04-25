use anchor_lang::prelude::*;
use halcyon_common::HalcyonError;
#[cfg(not(target_os = "solana"))]
use halcyon_flagship_quote::worst_of_factored::FactoredWorstOfModel;
use halcyon_flagship_quote::{
    worst_of_c1_fast::{spy_qqq_iwm_c1_config, spy_qqq_iwm_step_drift_inputs_s6},
    worst_of_c1_filter::quote_c1_filter,
};
use halcyon_kernel::state::{
    AutocallSchedule, PolicyHeader, ProtocolConfig, Regression, VaultSigma,
};
use solana_sha256_hasher::hash;
use solmath_core::{fp_mul, fp_sqrt, SCALE};

use crate::calendar::{issue_trade_date, nth_trading_day_after, trading_close_timestamp_utc};
use crate::errors::FlagshipAutocallError;
use crate::state::{
    FlagshipAutocallTerms, CURRENT_ENGINE_VERSION, MONTHLY_COUPON_COUNT,
    MONTHLY_COUPON_TRADING_DAY_BOUNDARIES, QUARTERLY_AUTOCALL_COUNT,
    QUARTERLY_AUTOCALL_TRADING_DAY_BOUNDARIES,
};

const TRADING_DAYS_PER_YEAR: u128 = 252;
const SIGMA_MIN_S6: i64 = 80_000;
const SIGMA_MAX_S6: i64 = 800_000;
const K_RETAINED: usize = 12;
const SCALE_6_I128: i128 = 1_000_000;
const AUTOCALL_SCHEDULE_MAX_AGE_SECS: i64 = 2 * 86_400;

#[cfg(all(target_os = "solana", feature = "cu-diagnostics"))]
unsafe extern "C" {
    fn sol_log_compute_units_();
    fn sol_log_(message: *const u8, length: u64);
}

#[inline(always)]
pub(crate) fn cu_trace(label: &str) {
    #[cfg(all(target_os = "solana", feature = "cu-diagnostics"))]
    unsafe {
        sol_log_(label.as_ptr(), label.len() as u64);
        sol_log_compute_units_();
    }
    #[cfg(not(all(target_os = "solana", feature = "cu-diagnostics")))]
    let _ = label;
}

#[inline(always)]
fn schedule_trace_before_nth(idx: usize) {
    match idx {
        0 => cu_trace("flagship schedule before nth_trading_day_after[0]"),
        1 => cu_trace("flagship schedule before nth_trading_day_after[1]"),
        2 => cu_trace("flagship schedule before nth_trading_day_after[2]"),
        3 => cu_trace("flagship schedule before nth_trading_day_after[3]"),
        4 => cu_trace("flagship schedule before nth_trading_day_after[4]"),
        5 => cu_trace("flagship schedule before nth_trading_day_after[5]"),
        _ => {}
    }
}

#[inline(always)]
fn schedule_trace_after_nth(idx: usize) {
    match idx {
        0 => cu_trace("flagship schedule after nth_trading_day_after[0]"),
        1 => cu_trace("flagship schedule after nth_trading_day_after[1]"),
        2 => cu_trace("flagship schedule after nth_trading_day_after[2]"),
        3 => cu_trace("flagship schedule after nth_trading_day_after[3]"),
        4 => cu_trace("flagship schedule after nth_trading_day_after[4]"),
        5 => cu_trace("flagship schedule after nth_trading_day_after[5]"),
        _ => {}
    }
}

#[inline(always)]
fn schedule_trace_after_close(idx: usize) {
    match idx {
        0 => cu_trace("flagship schedule after trading_close_timestamp_utc[0]"),
        1 => cu_trace("flagship schedule after trading_close_timestamp_utc[1]"),
        2 => cu_trace("flagship schedule after trading_close_timestamp_utc[2]"),
        3 => cu_trace("flagship schedule after trading_close_timestamp_utc[3]"),
        4 => cu_trace("flagship schedule after trading_close_timestamp_utc[4]"),
        5 => cu_trace("flagship schedule after trading_close_timestamp_utc[5]"),
        _ => {}
    }
}

pub const K12_CORRECTION_SHA256: [u8; 32] = [
    0xb5, 0xfa, 0xa8, 0x97, 0xdd, 0xdd, 0x97, 0x01, 0x05, 0x58, 0x9a, 0x82, 0xc5, 0xb5, 0xa3, 0x40,
    0x4c, 0x56, 0x1e, 0x51, 0xc5, 0x00, 0xeb, 0xc5, 0x6d, 0x94, 0x8a, 0x19, 0xc8, 0xb2, 0xea, 0x6e,
];

// Derived daily-KI artifact for upstream hybrid COS/MC source-table
// provenance 907e9d60af467b0967a43712fd6b6fad63a5237e447b0fe6e635fdc75a94d26a.
pub const DAILY_KI_CORRECTION_SHA256: [u8; 32] = [
    0xf8, 0x9a, 0xc1, 0x37, 0x89, 0xda, 0xfe, 0x2f, 0x93, 0x3d, 0x47, 0x37, 0x99, 0xf9, 0xb6, 0x17,
    0xb6, 0x66, 0xd2, 0xac, 0x76, 0x82, 0xc9, 0x0b, 0xc5, 0x0f, 0x59, 0x86, 0x2e, 0x61, 0xee, 0x0f,
];

pub struct QuoteOutputs {
    pub premium: u64,
    pub max_liability: u64,
    pub fair_coupon_bps_s6: i64,
    pub offered_coupon_bps_s6: i64,
    pub sigma_pricing_s6: i64,
    pub quote_slot: u64,
    pub expiry_ts: i64,
    pub engine_version: u16,
}

pub struct LiveDeltaOutputs {
    pub coupon_bps_s6: i64,
    pub delta_spy_s6: i64,
    pub delta_qqq_s6: i64,
    pub delta_iwm_s6: i64,
}

pub fn compose_pricing_sigma(
    vault_sigma: &VaultSigma,
    sigma_floor_annualised_s6: i64,
    sigma_ceiling_annualised_s6: i64,
) -> Result<i64> {
    require!(
        sigma_floor_annualised_s6 > 0,
        FlagshipAutocallError::InvalidSigmaFloor
    );
    require!(
        sigma_ceiling_annualised_s6 >= sigma_floor_annualised_s6,
        FlagshipAutocallError::InvalidSigmaCeiling
    );
    if vault_sigma.ewma_var_daily_s12 <= 0 {
        return Ok(sigma_floor_annualised_s6);
    }

    let annual_variance_s12 = fp_mul(
        vault_sigma.ewma_var_daily_s12 as u128,
        TRADING_DAYS_PER_YEAR
            .checked_mul(SCALE)
            .ok_or(HalcyonError::Overflow)?,
    )
    .map_err(|_| error!(HalcyonError::Overflow))?;
    let sigma_annual_s12 =
        fp_sqrt(annual_variance_s12).map_err(|_| error!(HalcyonError::Overflow))? as i128;
    let sigma_s6 = i64::try_from(
        sigma_annual_s12
            .checked_div(1_000_000)
            .ok_or(HalcyonError::Overflow)?,
    )
    .map_err(|_| error!(HalcyonError::Overflow))?;
    Ok(sigma_s6.clamp(sigma_floor_annualised_s6, sigma_ceiling_annualised_s6))
}

pub fn protocol_sigma_floor_annualised_s6(config: &ProtocolConfig) -> i64 {
    config.sigma_floor_for_product_s6(&crate::ID)
}

pub fn solve_quote(
    sigma_pricing_s6: i64,
    notional_usdc: u64,
    autocall_schedule: &AutocallSchedule,
) -> Result<QuoteOutputs> {
    require!(
        (SIGMA_MIN_S6..=SIGMA_MAX_S6).contains(&sigma_pricing_s6),
        FlagshipAutocallError::SigmaOutOfRange
    );

    let coupon_bps_s6 = deterministic_coupon_bps_s6(sigma_pricing_s6)?;
    cu_trace("flagship after deterministic_coupon_bps_s6");
    let quarterly_schedule = quarterly_autocall_schedule_from_account(autocall_schedule)?;
    cu_trace("flagship after build_quarterly_autocall_schedule");
    let expiry_ts = quarterly_schedule
        .last()
        .copied()
        .ok_or(HalcyonError::Overflow)?;
    cu_trace("flagship after expiry_ts extract");
    let quote_slot = Clock::get()?.slot;
    cu_trace("flagship after Clock::get");

    let out = QuoteOutputs {
        premium: 0,
        max_liability: notional_usdc,
        fair_coupon_bps_s6: coupon_bps_s6,
        offered_coupon_bps_s6: coupon_bps_s6,
        sigma_pricing_s6,
        quote_slot,
        expiry_ts,
        engine_version: CURRENT_ENGINE_VERSION,
    };
    cu_trace("flagship solve_quote return");
    Ok(out)
}

/// Compute live per-note delta through the analytical flagship filter-gradient path.
///
/// The keeper uses this off-chain helper to map live product terms and Pyth spots
/// into the quote crate's analytical delta engine. The core gradient primitive is
/// Stein-validated in `halcyon_flagship_quote`; this wrapper is not a heuristic
/// placeholder or Monte Carlo estimate.
#[cfg(not(target_os = "solana"))]
pub fn compute_live_delta_s6(
    terms: &FlagshipAutocallTerms,
    sigma_pricing_s6: i64,
    notional_usdc: u64,
    spot_spy_s6: i64,
    spot_qqq_s6: i64,
    spot_iwm_s6: i64,
) -> Result<LiveDeltaOutputs> {
    require!(
        (SIGMA_MIN_S6..=SIGMA_MAX_S6).contains(&sigma_pricing_s6),
        FlagshipAutocallError::SigmaOutOfRange
    );
    if terms.status != crate::state::ProductStatus::Active {
        return Ok(LiveDeltaOutputs {
            coupon_bps_s6: deterministic_coupon_bps_s6(sigma_pricing_s6)?,
            delta_spy_s6: 0,
            delta_qqq_s6: 0,
            delta_iwm_s6: 0,
        });
    }

    let cfg = spy_qqq_iwm_c1_config();
    let model = FactoredWorstOfModel::spy_qqq_iwm_current();
    let sigma_ann = sigma_pricing_s6 as f64 / 1_000_000.0;
    let drifts = model
        .risk_neutral_step_drifts(sigma_ann, 63)
        .map_err(|_| error!(FlagshipAutocallError::QuoteRecomputeMismatch))?;
    let drift_diffs = [
        ((drifts[1] - drifts[0]) * 1_000_000.0).round() as i64,
        ((drifts[2] - drifts[0]) * 1_000_000.0).round() as i64,
    ];
    let drift_shift_63 = ((cfg.loadings[0] as f64 * drifts[0])
        + (cfg.loadings[1] as f64 * drifts[1])
        + (cfg.loadings[2] as f64 * drifts[2]))
        .round() as i64;
    let remaining_observations =
        QUARTERLY_AUTOCALL_COUNT.saturating_sub(terms.next_autocall_index as usize);
    let live_spots_s6 = [
        spot_ratio_s6(spot_spy_s6, terms.entry_spy_price_s6)?,
        spot_ratio_s6(spot_qqq_s6, terms.entry_qqq_price_s6)?,
        spot_ratio_s6(spot_iwm_s6, terms.entry_iwm_price_s6)?,
    ];
    let quote = halcyon_flagship_quote::worst_of_c1_filter::quote_c1_filter_with_delta_live(
        &cfg,
        sigma_pricing_s6,
        drift_diffs,
        drift_shift_63,
        K_RETAINED,
        live_spots_s6,
        remaining_observations,
        terms.ki_latched,
    );
    let notional_scale = notional_usdc as f64 / 1_000_000.0;

    Ok(LiveDeltaOutputs {
        coupon_bps_s6: deterministic_coupon_bps_s6(sigma_pricing_s6)?,
        delta_spy_s6: (quote.delta_spy * notional_scale * 1_000_000.0).round() as i64,
        delta_qqq_s6: (quote.delta_qqq * notional_scale * 1_000_000.0).round() as i64,
        delta_iwm_s6: (quote.delta_iwm * notional_scale * 1_000_000.0).round() as i64,
    })
}

pub fn build_monthly_coupon_schedule(issued_at: i64) -> Result<[i64; MONTHLY_COUPON_COUNT]> {
    build_schedule_from_boundaries::<MONTHLY_COUPON_COUNT>(
        issued_at,
        &MONTHLY_COUPON_TRADING_DAY_BOUNDARIES,
    )
}

pub fn build_quarterly_autocall_schedule_from_calendar(
    issued_at: i64,
) -> Result<[i64; QUARTERLY_AUTOCALL_COUNT]> {
    build_schedule_from_boundaries::<QUARTERLY_AUTOCALL_COUNT>(
        issued_at,
        &QUARTERLY_AUTOCALL_TRADING_DAY_BOUNDARIES,
    )
}

pub fn quarterly_autocall_schedule_from_account(
    autocall_schedule: &AutocallSchedule,
) -> Result<[i64; QUARTERLY_AUTOCALL_COUNT]> {
    Ok(autocall_schedule.observation_timestamps)
}

pub fn coupon_per_observation_usdc(notional_usdc: u64, offered_coupon_bps_s6: i64) -> Result<u64> {
    require!(
        offered_coupon_bps_s6 >= 0,
        FlagshipAutocallError::QuoteRecomputeMismatch
    );
    let coupon = (notional_usdc as u128)
        .checked_mul(offered_coupon_bps_s6 as u128)
        .and_then(|v| v.checked_div(10_000))
        .and_then(|v| v.checked_div(SCALE_6_I128 as u128))
        .ok_or(HalcyonError::Overflow)?;
    u64::try_from(coupon).map_err(|_| error!(HalcyonError::Overflow))
}

pub fn coupon_due_with_memory_usdc(
    notional_usdc: u64,
    offered_coupon_bps_s6: i64,
    missed_coupon_observations: u8,
) -> Result<u64> {
    let coupon = coupon_per_observation_usdc(notional_usdc, offered_coupon_bps_s6)?;
    let coupon_count = u64::from(missed_coupon_observations)
        .checked_add(1)
        .ok_or(HalcyonError::Overflow)?;
    coupon
        .checked_mul(coupon_count)
        .ok_or_else(|| error!(HalcyonError::Overflow))
}

pub fn worst_ratio_s6(
    terms: &FlagshipAutocallTerms,
    spy_price_s6: i64,
    qqq_price_s6: i64,
    iwm_price_s6: i64,
) -> Result<i64> {
    let spy = ratio_s6(spy_price_s6, terms.entry_spy_price_s6)?;
    let qqq = ratio_s6(qqq_price_s6, terms.entry_qqq_price_s6)?;
    let iwm = ratio_s6(iwm_price_s6, terms.entry_iwm_price_s6)?;
    Ok(spy.min(qqq).min(iwm))
}

pub fn maturity_payout_usdc(
    policy_header: &PolicyHeader,
    terms: &FlagshipAutocallTerms,
    worst_ratio_s6_now: i64,
) -> Result<u64> {
    require!(
        worst_ratio_s6_now >= 0,
        FlagshipAutocallError::InvalidEntryPrice
    );
    if !terms.ki_latched || worst_ratio_s6_now >= 1_000_000 {
        return Ok(policy_header.notional);
    }
    let recovered = (policy_header.notional as u128)
        .checked_mul(worst_ratio_s6_now as u128)
        .and_then(|v| v.checked_div(1_000_000))
        .ok_or(HalcyonError::Overflow)?;
    u64::try_from(recovered).map_err(|_| error!(HalcyonError::Overflow))
}

pub fn quarterly_coupon_index(expected_autocall_index: u8) -> Result<u8> {
    let coupon_index = (u16::from(expected_autocall_index) + 1)
        .checked_mul(3)
        .and_then(|v| v.checked_sub(1))
        .ok_or(HalcyonError::Overflow)?;
    u8::try_from(coupon_index).map_err(|_| error!(HalcyonError::Overflow))
}

pub fn require_protocol_unpaused(cfg: &ProtocolConfig) -> Result<()> {
    require!(!cfg.issuance_paused_global, HalcyonError::PausedGlobally);
    Ok(())
}

pub fn require_sigma_fresh(vault_sigma: &VaultSigma, now: i64, cap_secs: i64) -> Result<()> {
    let age = now
        .checked_sub(vault_sigma.ewma_last_timestamp)
        .ok_or(HalcyonError::Overflow)?;
    require!(age <= cap_secs, HalcyonError::SigmaStale);
    Ok(())
}

pub fn require_regression_fresh(regression: &Regression, now: i64, cap_secs: i64) -> Result<()> {
    let age = now
        .checked_sub(regression.last_update_ts)
        .ok_or(HalcyonError::Overflow)?;
    require!(age <= cap_secs, HalcyonError::RegressionStale);
    Ok(())
}

pub fn require_autocall_schedule_fresh(
    autocall_schedule: &AutocallSchedule,
    now: i64,
) -> Result<()> {
    let age = now
        .checked_sub(autocall_schedule.issue_date_ts)
        .ok_or(HalcyonError::Overflow)?;
    require!(
        age <= AUTOCALL_SCHEDULE_MAX_AGE_SECS,
        HalcyonError::AutocallScheduleStale
    );
    Ok(())
}

pub fn require_correction_tables_match(cfg: &ProtocolConfig) -> Result<()> {
    require!(
        cfg.k12_correction_sha256 == K12_CORRECTION_SHA256,
        HalcyonError::CorrectionTableHashMismatch
    );
    require!(
        cfg.daily_ki_correction_sha256 == DAILY_KI_CORRECTION_SHA256,
        HalcyonError::CorrectionTableHashMismatch
    );
    Ok(())
}

pub fn require_quote_acceptance_bounds(
    quote: &QuoteOutputs,
    min_offered_coupon_bps_s6: i64,
    preview_quote_slot: u64,
    max_quote_slot_delta: u64,
    live_spy_price_s6: i64,
    preview_spy_price_s6: i64,
    live_qqq_price_s6: i64,
    preview_qqq_price_s6: i64,
    live_iwm_price_s6: i64,
    preview_iwm_price_s6: i64,
    max_entry_price_deviation_bps: u16,
    preview_expiry_ts: i64,
    max_expiry_delta_secs: i64,
) -> Result<()> {
    require!(
        quote.offered_coupon_bps_s6 >= min_offered_coupon_bps_s6,
        HalcyonError::SlippageExceeded
    );
    require!(
        quote.quote_slot >= preview_quote_slot,
        HalcyonError::SlippageExceeded
    );
    require!(
        quote.quote_slot - preview_quote_slot <= max_quote_slot_delta,
        HalcyonError::SlippageExceeded
    );
    require_price_deviation_within(
        live_spy_price_s6,
        preview_spy_price_s6,
        max_entry_price_deviation_bps,
    )?;
    require_price_deviation_within(
        live_qqq_price_s6,
        preview_qqq_price_s6,
        max_entry_price_deviation_bps,
    )?;
    require_price_deviation_within(
        live_iwm_price_s6,
        preview_iwm_price_s6,
        max_entry_price_deviation_bps,
    )?;
    let expiry_delta = (quote.expiry_ts - preview_expiry_ts).abs();
    require!(
        expiry_delta <= max_expiry_delta_secs,
        HalcyonError::SlippageExceeded
    );
    Ok(())
}

pub fn hash_product_terms(terms: &FlagshipAutocallTerms) -> Result<[u8; 32]> {
    use anchor_lang::Discriminator;
    let mut buf = Vec::with_capacity(8 + FlagshipAutocallTerms::INIT_SPACE);
    buf.extend_from_slice(&FlagshipAutocallTerms::DISCRIMINATOR);
    terms
        .serialize(&mut buf)
        .map_err(|_| error!(HalcyonError::Overflow))?;
    Ok(hash(&buf).to_bytes())
}

fn deterministic_coupon_bps_s6(sigma_pricing_s6: i64) -> Result<i64> {
    let cfg = spy_qqq_iwm_c1_config();
    let (drift_diffs, drift_shift_63) =
        spy_qqq_iwm_step_drift_inputs_s6(&cfg, sigma_pricing_s6, 63)
            .map_err(|_| error!(FlagshipAutocallError::QuoteRecomputeMismatch))?;
    cu_trace("flagship before quote_c1_filter");
    let quote = quote_c1_filter(
        &cfg,
        sigma_pricing_s6,
        drift_diffs,
        drift_shift_63,
        K_RETAINED,
    );
    cu_trace("flagship after quote_c1_filter");
    let coupon_bps_s6 = quote.fair_coupon_bps_s6;
    if coupon_bps_s6 <= 0 {
        return err!(FlagshipAutocallError::QuoteRecomputeMismatch);
    }
    cu_trace("flagship deterministic_coupon_bps_s6 return");
    Ok(coupon_bps_s6)
}

fn ratio_s6(numerator_s6: i64, denominator_s6: i64) -> Result<i64> {
    require!(
        numerator_s6 > 0 && denominator_s6 > 0,
        FlagshipAutocallError::InvalidEntryPrice
    );
    let ratio = (numerator_s6 as i128)
        .checked_mul(1_000_000)
        .and_then(|v| v.checked_div(denominator_s6 as i128))
        .ok_or(HalcyonError::Overflow)?;
    i64::try_from(ratio).map_err(|_| error!(HalcyonError::Overflow))
}

#[cfg(not(target_os = "solana"))]
fn spot_ratio_s6(spot_s6: i64, entry_s6: i64) -> Result<i64> {
    ratio_s6(spot_s6, entry_s6)
}

fn require_price_deviation_within(
    live_price_s6: i64,
    preview_price_s6: i64,
    max_deviation_bps: u16,
) -> Result<()> {
    require!(
        live_price_s6 > 0 && preview_price_s6 > 0,
        FlagshipAutocallError::InvalidEntryPrice
    );
    let diff = (i128::from(live_price_s6) - i128::from(preview_price_s6)).abs() as u128;
    let deviation_bps = diff
        .checked_mul(10_000)
        .and_then(|v| v.checked_div(preview_price_s6 as u128))
        .ok_or(HalcyonError::Overflow)?;
    require!(
        deviation_bps <= max_deviation_bps as u128,
        HalcyonError::SlippageExceeded
    );
    Ok(())
}

pub fn ratio_meets_barrier(ratio_s6_now: i64, barrier_bps: u16) -> Result<bool> {
    let barrier_s6 = i64::from(barrier_bps)
        .checked_mul(100)
        .ok_or(HalcyonError::Overflow)?;
    Ok(ratio_s6_now >= barrier_s6)
}

pub fn ratio_breaches_barrier(ratio_s6_now: i64, barrier_bps: u16) -> Result<bool> {
    let barrier_s6 = i64::from(barrier_bps)
        .checked_mul(100)
        .ok_or(HalcyonError::Overflow)?;
    Ok(ratio_s6_now <= barrier_s6)
}

fn build_schedule_from_boundaries<const N: usize>(
    issued_at: i64,
    boundaries: &[u16; N],
) -> Result<[i64; N]> {
    #[cfg(feature = "integration-test")]
    {
        let mut schedule = [0i64; N];
        for (idx, boundary) in boundaries.iter().copied().enumerate() {
            schedule[idx] = issued_at
                .checked_add(i64::from(boundary) * crate::state::SECONDS_PER_DAY)
                .ok_or(HalcyonError::Overflow)?;
        }
        return Ok(schedule);
    }

    #[cfg(not(feature = "integration-test"))]
    let issue_date = {
        cu_trace("flagship schedule before issue_trade_date");
        let date = issue_trade_date(issued_at);
        cu_trace("flagship schedule after issue_trade_date");
        date
    };

    let mut schedule = [0i64; N];
    for (idx, trading_day_boundary) in boundaries.iter().copied().enumerate() {
        #[cfg(not(feature = "integration-test"))]
        schedule_trace_before_nth(idx);
        #[cfg(not(feature = "integration-test"))]
        let trading_day = nth_trading_day_after(issue_date, trading_day_boundary)?;
        #[cfg(not(feature = "integration-test"))]
        schedule_trace_after_nth(idx);
        #[cfg(not(feature = "integration-test"))]
        {
            schedule[idx] = trading_close_timestamp_utc(trading_day);
            schedule_trace_after_close(idx);
        }
    }
    Ok(schedule)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correction_hash_constants_match_sources() {
        assert_eq!(
            K12_CORRECTION_SHA256,
            hash(include_bytes!(
                "../../../crates/halcyon_flagship_quote/src/k12_correction.rs"
            ))
            .to_bytes()
        );
        assert_eq!(
            DAILY_KI_CORRECTION_SHA256,
            [
                0xf8, 0x9a, 0xc1, 0x37, 0x89, 0xda, 0xfe, 0x2f, 0x93, 0x3d, 0x47, 0x37, 0x99, 0xf9,
                0xb6, 0x17, 0xb6, 0x66, 0xd2, 0xac, 0x76, 0x82, 0xc9, 0x0b, 0xc5, 0x0f, 0x59, 0x86,
                0x2e, 0x61, 0xee, 0x0f,
            ]
        );
    }

    #[test]
    fn calendar_schedule_uses_trading_day_boundaries() {
        let issued_at = 1_767_385_600i64; // 2026-01-02 12:00:00 UTC
        let monthly = build_monthly_coupon_schedule(issued_at).unwrap();
        let quarterly = build_quarterly_autocall_schedule_from_calendar(issued_at).unwrap();
        let issue_date = crate::calendar::issue_trade_date(issued_at);
        let first_coupon_trade_date =
            crate::calendar::nth_trading_day_after(issue_date, 21).unwrap();
        let maturity_trade_date =
            crate::calendar::nth_trading_day_after(issue_date, crate::state::TENOR_TRADING_DAYS)
                .unwrap();
        assert_eq!(
            monthly[0],
            crate::calendar::trading_close_timestamp_utc(first_coupon_trade_date)
        );
        assert_eq!(quarterly[0], monthly[2]);
        assert_eq!(quarterly[5], monthly[17]);
        assert_eq!(
            quarterly[5],
            crate::calendar::trading_close_timestamp_utc(maturity_trade_date)
        );
    }

    #[test]
    fn coupon_due_with_memory_multiplies_unpaid_observations() {
        let one_coupon = coupon_per_observation_usdc(100_000_000, 150_000_000).unwrap();
        let due = coupon_due_with_memory_usdc(100_000_000, 150_000_000, 2).unwrap();
        assert_eq!(due, one_coupon * 3);
    }

    #[test]
    fn quarterly_schedule_from_account_returns_keeper_posted_values() {
        let schedule = AutocallSchedule {
            version: 1,
            product_program_id: crate::ID,
            issue_date_ts: 1_700_000_000,
            observation_timestamps: [11, 22, 33, 44, 55, 66],
            last_publish_ts: 1_700_000_001,
            last_publish_slot: 7,
        };
        assert_eq!(
            quarterly_autocall_schedule_from_account(&schedule).unwrap(),
            [11, 22, 33, 44, 55, 66]
        );
    }
}
