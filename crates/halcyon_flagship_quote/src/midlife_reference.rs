//! Host-side mid-life NAV reference for the SPY/QQQ/IWM flagship autocall.
//!
//! This entrypoint exists so the fixture generator can pin a named
//! `nav_c1_filter_mid_life` reference function while staying on the locked GH9
//! c1-filter path. It intentionally does not route through the older
//! `worst_of_factored` surrogate, which is a different model family and not the
//! Phase C parity target.

use crate::midlife_pricer::{compute_midlife_nav, MidlifeInputs, MidlifeNav, MidlifePricerError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidlifeReferenceError {
    Pricer(MidlifePricerError),
}

impl From<MidlifePricerError> for MidlifeReferenceError {
    fn from(value: MidlifePricerError) -> Self {
        Self::Pricer(value)
    }
}

/// Compute the host-side mid-life NAV reference from live flagship inputs.
///
/// The fixture contract keeps this symbol stable, but the math is the same
/// c1-filter implementation used by `compute_midlife_nav`.
pub fn nav_c1_filter_mid_life(inputs: &MidlifeInputs) -> Result<MidlifeNav, MidlifeReferenceError> {
    compute_midlife_nav(inputs).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midlife_pricer::compute_midlife_nav;

    fn sample_inputs() -> MidlifeInputs {
        MidlifeInputs {
            current_spy_s6: 100_000_000,
            current_qqq_s6: 100_000_000,
            current_iwm_s6: 100_000_000,
            sigma_common_s6: 180_000,
            entry_spy_s6: 100_000_000,
            entry_qqq_s6: 100_000_000,
            entry_iwm_s6: 100_000_000,
            beta_spy_s12: 1_000_000_000_000,
            beta_qqq_s12: 0,
            alpha_s12: 0,
            regression_residual_vol_s6: 0,
            monthly_coupon_schedule: [
                21, 42, 63, 84, 105, 126, 147, 168, 189, 210, 231, 252, 273, 294, 315, 336, 357,
                378,
            ],
            quarterly_autocall_schedule: [63, 126, 189, 252, 315, 378],
            next_coupon_index: 0,
            next_autocall_index: 0,
            offered_coupon_bps_s6: 500_000_000,
            coupon_barrier_bps: 10_000,
            autocall_barrier_bps: 10_000,
            ki_barrier_bps: 8_000,
            ki_latched: false,
            missed_coupon_observations: 0,
            coupons_paid_usdc: 0,
            notional_usdc: 100_000_000,
            now_trading_day: 0,
        }
    }

    #[test]
    fn final_day_without_ki_returns_par() {
        let mut inputs = sample_inputs();
        inputs.current_spy_s6 = 95_000_000;
        inputs.current_qqq_s6 = 94_000_000;
        inputs.current_iwm_s6 = 93_000_000;
        inputs.next_coupon_index = 17;
        inputs.next_autocall_index = 5;
        inputs.now_trading_day = 378;

        let nav = nav_c1_filter_mid_life(&inputs).expect("final-day nav");
        assert_eq!(nav.nav_s6, 1_000_000);
        assert_eq!(nav.remaining_coupon_pv_s6, 0);
        assert_eq!(nav.par_recovery_probability_s6, 1_000_000);
    }

    #[test]
    fn host_reference_matches_midlife_pricer_for_memory_state() {
        let mut inputs = sample_inputs();
        inputs.current_spy_s6 = 120_000_000;
        inputs.current_qqq_s6 = 118_000_000;
        inputs.current_iwm_s6 = 121_000_000;
        inputs.next_coupon_index = 2;
        inputs.next_autocall_index = 0;
        inputs.now_trading_day = 63;
        inputs.missed_coupon_observations = 2;

        let reference = nav_c1_filter_mid_life(&inputs).expect("reference nav");
        let pricer = compute_midlife_nav(&inputs).expect("midlife pricer nav");
        assert_eq!(reference, pricer);
    }

    #[test]
    fn deep_itm_start_counts_coupon_only_months_before_autocall() {
        let mut inputs = sample_inputs();
        inputs.current_spy_s6 = 120_000_000;
        inputs.current_qqq_s6 = 118_000_000;
        inputs.current_iwm_s6 = 122_000_000;
        inputs.sigma_common_s6 = 1;

        let nav = nav_c1_filter_mid_life(&inputs).expect("deep itm start nav");
        assert!(
            nav.remaining_coupon_pv_s6 > 140_000,
            "expected three coupon observations before autocall, got {}",
            nav.remaining_coupon_pv_s6
        );
        assert!(
            nav.nav_s6 > 1_140_000,
            "expected near-par plus three coupons, got {}",
            nav.nav_s6
        );
        assert!(nav.par_recovery_probability_s6 >= 999_000);
    }

    #[test]
    fn final_day_knocked_note_tracks_worst_ratio() {
        let mut inputs = sample_inputs();
        inputs.current_spy_s6 = 52_000_000;
        inputs.current_qqq_s6 = 50_000_000;
        inputs.current_iwm_s6 = 51_000_000;
        inputs.next_coupon_index = 17;
        inputs.next_autocall_index = 5;
        inputs.now_trading_day = 378;
        inputs.ki_latched = true;

        let nav = nav_c1_filter_mid_life(&inputs).expect("knocked nav");
        assert!((nav.nav_s6 - 500_000).abs() <= 1);
        assert_eq!(nav.remaining_coupon_pv_s6, 0);
        assert_eq!(nav.par_recovery_probability_s6, 0);
        assert_eq!(nav.ki_level_usd_s6, 800_000);
    }
}
