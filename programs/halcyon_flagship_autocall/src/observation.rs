use anchor_lang::prelude::*;

use halcyon_common::HalcyonError;
use halcyon_kernel::state::PolicyHeader;
use halcyon_oracles::PriceSnapshot;

use crate::errors::FlagshipAutocallError;
use crate::pricing::{coupon_due_with_memory_usdc, ratio_meets_barrier, worst_ratio_s6};
use crate::state::{FlagshipAutocallTerms, MONTHLY_COUPON_COUNT, QUARTERLY_AUTOCALL_COUNT};

pub const OBSERVATION_WINDOW_LEAD_SECS: i64 = 5 * 60;
pub const OBSERVATION_WINDOW_LAG_SECS: i64 = 15 * 60;
pub const OBSERVATION_SNAPSHOT_MAX_SKEW_SECS: i64 = 60;

pub fn observation_window_bounds(scheduled_ts: i64) -> Result<(i64, i64)> {
    let start = scheduled_ts
        .checked_sub(OBSERVATION_WINDOW_LEAD_SECS)
        .ok_or(HalcyonError::Overflow)?;
    let end = scheduled_ts
        .checked_add(OBSERVATION_WINDOW_LAG_SECS)
        .ok_or(HalcyonError::Overflow)?;
    Ok((start, end))
}

pub fn read_equity_observation_worst_ratio_s6(
    terms: &FlagshipAutocallTerms,
    scheduled_ts: i64,
    pyth_spy: &AccountInfo,
    pyth_qqq: &AccountInfo,
    pyth_iwm: &AccountInfo,
) -> Result<i64> {
    let (window_start, window_end) = observation_window_bounds(scheduled_ts)?;
    let spy = halcyon_oracles::read_pyth_price_in_range(
        pyth_spy,
        &halcyon_oracles::feed_ids::SPY_USD,
        &crate::ID,
        window_start,
        window_end,
    )?;
    let qqq = halcyon_oracles::read_pyth_price_in_range(
        pyth_qqq,
        &halcyon_oracles::feed_ids::QQQ_USD,
        &crate::ID,
        window_start,
        window_end,
    )?;
    let iwm = halcyon_oracles::read_pyth_price_in_range(
        pyth_iwm,
        &halcyon_oracles::feed_ids::IWM_USD,
        &crate::ID,
        window_start,
        window_end,
    )?;
    require_snapshot_sync(&[spy, qqq, iwm])?;
    worst_ratio_s6(terms, spy.price_s6, qqq.price_s6, iwm.price_s6)
}

pub fn coupon_outcome(
    policy_header: &PolicyHeader,
    terms: &FlagshipAutocallTerms,
    worst_ratio_s6_now: i64,
) -> Result<(bool, u64)> {
    let should_pay = ratio_meets_barrier(worst_ratio_s6_now, terms.coupon_barrier_bps)?;
    let coupon_due = if should_pay {
        coupon_due_with_memory_usdc(
            policy_header.notional,
            terms.offered_coupon_bps_s6,
            terms.missed_coupon_observations,
        )?
    } else {
        0
    };
    Ok((should_pay, coupon_due))
}

pub fn commit_coupon_observation(
    terms: &mut FlagshipAutocallTerms,
    expected_index: u8,
    should_pay: bool,
    coupon_due: u64,
) -> Result<()> {
    terms.next_coupon_index = expected_index.saturating_add(1);
    if should_pay {
        terms.missed_coupon_observations = 0;
    } else {
        terms.missed_coupon_observations = terms
            .missed_coupon_observations
            .checked_add(1)
            .ok_or(HalcyonError::Overflow)?;
    }
    terms.coupons_paid_usdc = terms
        .coupons_paid_usdc
        .checked_add(coupon_due)
        .ok_or(HalcyonError::Overflow)?;
    Ok(())
}

pub fn require_coupon_reconciled_through(
    terms: &FlagshipAutocallTerms,
    coupon_index: u8,
) -> Result<()> {
    require!(
        terms.next_coupon_index > coupon_index,
        FlagshipAutocallError::ObservationReconciliationRequired
    );
    Ok(())
}

pub fn require_all_coupon_observations_reconciled(terms: &FlagshipAutocallTerms) -> Result<()> {
    require!(
        terms.next_coupon_index as usize == MONTHLY_COUPON_COUNT,
        FlagshipAutocallError::ObservationReconciliationRequired
    );
    Ok(())
}

pub fn require_all_autocall_observations_reconciled(terms: &FlagshipAutocallTerms) -> Result<()> {
    require!(
        terms.next_autocall_index as usize == QUARTERLY_AUTOCALL_COUNT,
        FlagshipAutocallError::ObservationReconciliationRequired
    );
    Ok(())
}

pub fn require_snapshot_sync(snapshots: &[PriceSnapshot; 3]) -> Result<()> {
    let earliest = snapshots
        .iter()
        .map(|snapshot| snapshot.publish_ts)
        .min()
        .ok_or(HalcyonError::Overflow)?;
    let latest = snapshots
        .iter()
        .map(|snapshot| snapshot.publish_ts)
        .max()
        .ok_or(HalcyonError::Overflow)?;
    require!(
        latest.checked_sub(earliest).ok_or(HalcyonError::Overflow)?
            <= OBSERVATION_SNAPSHOT_MAX_SKEW_SECS,
        FlagshipAutocallError::ObservationSnapshotSkewed
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ProductStatus;

    fn sample_terms() -> FlagshipAutocallTerms {
        FlagshipAutocallTerms {
            version: FlagshipAutocallTerms::CURRENT_VERSION,
            policy_header: Pubkey::new_unique(),
            entry_spy_price_s6: 100_000_000,
            entry_qqq_price_s6: 100_000_000,
            entry_iwm_price_s6: 100_000_000,
            monthly_coupon_schedule: [0; MONTHLY_COUPON_COUNT],
            quarterly_autocall_schedule: [0; QUARTERLY_AUTOCALL_COUNT],
            next_coupon_index: 2,
            next_autocall_index: 1,
            offered_coupon_bps_s6: 850_000,
            coupon_barrier_bps: 10_000,
            autocall_barrier_bps: 10_000,
            ki_barrier_bps: 8_000,
            missed_coupon_observations: 1,
            ki_latched: false,
            coupons_paid_usdc: 10,
            beta_spy_s12: 0,
            beta_qqq_s12: 0,
            alpha_s12: 0,
            regression_r_squared_s6: 0,
            regression_residual_vol_s6: 0,
            k12_correction_sha256: [0; 32],
            daily_ki_correction_sha256: [0; 32],
            settled_payout_usdc: 0,
            settled_at: 0,
            status: ProductStatus::Active,
        }
    }

    #[test]
    fn commit_coupon_observation_resets_memory_after_payment() {
        let mut terms = sample_terms();
        commit_coupon_observation(&mut terms, 2, true, 25).unwrap();
        assert_eq!(terms.next_coupon_index, 3);
        assert_eq!(terms.missed_coupon_observations, 0);
        assert_eq!(terms.coupons_paid_usdc, 35);
    }

    #[test]
    fn commit_coupon_observation_tracks_new_miss() {
        let mut terms = sample_terms();
        commit_coupon_observation(&mut terms, 2, false, 0).unwrap();
        assert_eq!(terms.next_coupon_index, 3);
        assert_eq!(terms.missed_coupon_observations, 2);
        assert_eq!(terms.coupons_paid_usdc, 10);
    }

    #[test]
    fn quarterly_paths_reject_when_coupon_state_is_behind() {
        let mut terms = sample_terms();
        terms.next_coupon_index = 2;
        let err = require_coupon_reconciled_through(&terms, 2).unwrap_err();
        assert_eq!(
            err,
            error!(FlagshipAutocallError::ObservationReconciliationRequired)
        );
    }

    #[test]
    fn settlement_requires_all_coupon_and_autocall_observations() {
        let terms = sample_terms();
        assert_eq!(
            require_all_coupon_observations_reconciled(&terms).unwrap_err(),
            error!(FlagshipAutocallError::ObservationReconciliationRequired)
        );
        assert_eq!(
            require_all_autocall_observations_reconciled(&terms).unwrap_err(),
            error!(FlagshipAutocallError::ObservationReconciliationRequired)
        );
    }
}
