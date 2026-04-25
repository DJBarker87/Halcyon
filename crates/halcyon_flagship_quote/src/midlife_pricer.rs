//! Mid-life NAV pricer for the SPY/QQQ/IWM flagship autocall.
//!
//! Fixed-point mid-life NAV pricer for the on-chain flagship buyback flow.
//!
//! The production path uses the explicit monthly c1-filter dynamic program.
//! Checkpoints carry deterministic frontier state so expensive observations
//! can be split across transactions without applying any NAV correction term.
//!
//! Quadrature lock (decision 2026-04-23): the on-chain port uses `GH9_NODES_S6`
//! + `nig_importance_weights_9`, matching the existing on-chain
//! `quote_c1_filter`. No GH13 fixed-point conversion.
//!
//! Compiles on both host and `target_os = "solana"`. Derives `Serialize` /
//! `Deserialize` unconditionally to support Phase B fixture roundtrip; the
//! pattern matches the existing `worst_of_factored` structs.

pub use crate::worst_of_c1_filter::{
    MidlifeCheckpointNode, MidlifeCheckpointState, MidlifeNavCheckpoint, MIDLIFE_CHECKPOINT_BYTES,
    MIDLIFE_CHECKPOINT_K, MIDLIFE_CHECKPOINT_MEMORY_BUCKETS, MIDLIFE_CHECKPOINT_NODE_BYTES,
    MIDLIFE_CHECKPOINT_STATE_BYTES, MIDLIFE_CHECKPOINT_VERSION,
};
use serde::{Deserialize, Serialize};

/// Inputs to a single mid-life NAV evaluation.
///
/// All scalars are fixed-point, suffix indicates the scale (`_s6` =
/// `* 1_000_000`, `_s12` = `* 10^12`, `_bps` = basis-point integer with no
/// further scaling unless suffixed). Schedules are in trading-day indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidlifeInputs {
    // ---- Current market state (fresh Pyth in-tx) ----
    pub current_spy_s6: i64,
    /// Synthetic if projected via regression from SPY/QQQ.
    pub current_qqq_s6: i64,
    pub current_iwm_s6: i64,
    pub sigma_common_s6: i64,

    // ---- Entry state (from ProductTerms) ----
    pub entry_spy_s6: i64,
    pub entry_qqq_s6: i64,
    pub entry_iwm_s6: i64,

    // ---- Regression (from kernel Regression PDA) ----
    pub beta_spy_s12: i128,
    pub beta_qqq_s12: i128,
    pub alpha_s12: i128,
    pub regression_residual_vol_s6: i64,

    // ---- Schedule / state (from ProductTerms) ----
    pub monthly_coupon_schedule: [i64; 18],
    pub quarterly_autocall_schedule: [i64; 6],
    pub next_coupon_index: u8,
    pub next_autocall_index: u8,
    pub offered_coupon_bps_s6: i64,
    pub coupon_barrier_bps: u16,
    pub autocall_barrier_bps: u16,
    pub ki_barrier_bps: u16,
    pub ki_latched: bool,
    pub missed_coupon_observations: u8,
    pub coupons_paid_usdc: u64,
    pub notional_usdc: u64,

    // ---- Time ----
    pub now_trading_day: u16,
}

/// Output of a mid-life NAV evaluation.
///
/// `nav_s6` is the present value of the remaining payoff per $1 notional,
/// scaled by 10^6. Diagnostics are returned alongside for observability and
/// for the buyback formula.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidlifeNav {
    /// Present value of remaining payoff, per $1 notional, SCALE_6.
    pub nav_s6: i64,
    /// Notional-denominated KI barrier in USD. Used by the buyback formula.
    pub ki_level_usd_s6: i64,
    /// Diagnostic: sum of discounted future coupons.
    pub remaining_coupon_pv_s6: i64,
    /// Diagnostic: probability of redeeming at par.
    pub par_recovery_probability_s6: i64,
}

/// Error returned by [`compute_midlife_nav`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MidlifePricerError {
    /// Reserved for unfinished branches during incremental rollout.
    NotImplemented,
    /// Inputs are malformed or outside the supported contract domain.
    InvalidInput,
    /// Underlying fixed-point math helper failed.
    MathError,
}

/// Compute the mid-life NAV of a single flagship policy.
pub fn compute_midlife_nav(inputs: &MidlifeInputs) -> Result<MidlifeNav, MidlifePricerError> {
    crate::worst_of_c1_filter::compute_midlife_nav_c1_filter(inputs)
}

/// Host/debug reference for the explicit 18 monthly-observation dynamic
/// program.
#[cfg(not(target_os = "solana"))]
pub fn compute_midlife_nav_monthly_debug(
    inputs: &MidlifeInputs,
) -> Result<MidlifeNav, MidlifePricerError> {
    crate::worst_of_c1_filter::compute_midlife_nav_monthly_c1_filter(inputs)
}

/// Compute a deterministic prefix of the monthly debug dynamic program.
///
/// The returned checkpoint contains live pricing state only. It does not
/// contain, or derive from, a reference NAV correction.
#[cfg(not(target_os = "solana"))]
pub fn start_midlife_nav(
    inputs: &MidlifeInputs,
    stop_coupon_index: u8,
) -> Result<MidlifeNavCheckpoint, MidlifePricerError> {
    crate::worst_of_c1_filter::start_midlife_nav_c1_filter(inputs, stop_coupon_index)
}

/// Resume a deterministic monthly debug checkpoint and finish the computation.
#[cfg(not(target_os = "solana"))]
pub fn finish_midlife_nav(
    inputs: &MidlifeInputs,
    checkpoint: &MidlifeNavCheckpoint,
) -> Result<MidlifeNav, MidlifePricerError> {
    crate::worst_of_c1_filter::finish_midlife_nav_c1_filter(inputs, checkpoint)
}

/// Advance a deterministic monthly debug checkpoint without finishing the NAV.
#[cfg(not(target_os = "solana"))]
pub fn advance_midlife_nav(
    inputs: &MidlifeInputs,
    checkpoint: &MidlifeNavCheckpoint,
    stop_coupon_index: u8,
) -> Result<MidlifeNavCheckpoint, MidlifePricerError> {
    crate::worst_of_c1_filter::advance_midlife_nav_c1_filter(inputs, checkpoint, stop_coupon_index)
}

/// Compute a deterministic prefix of the production monthly dynamic program
/// into a fixed byte buffer.
pub fn start_midlife_nav_into(
    inputs: &MidlifeInputs,
    stop_coupon_index: u8,
    checkpoint_out: &mut [u8],
) -> Result<(), MidlifePricerError> {
    crate::worst_of_c1_filter::start_midlife_nav_c1_filter_into(
        inputs,
        stop_coupon_index,
        checkpoint_out,
    )
}

/// Advance a deterministic production checkpoint in-place without finishing the NAV.
pub fn advance_midlife_nav_in_place(
    inputs: &MidlifeInputs,
    checkpoint: &mut [u8],
    stop_coupon_index: u8,
) -> Result<(), MidlifePricerError> {
    crate::worst_of_c1_filter::advance_midlife_nav_c1_filter_in_place(
        inputs,
        checkpoint,
        stop_coupon_index,
    )
}

/// Resume a deterministic production byte checkpoint and finish the NAV.
pub fn finish_midlife_nav_from_bytes(
    inputs: &MidlifeInputs,
    checkpoint: &[u8],
) -> Result<MidlifeNav, MidlifePricerError> {
    crate::worst_of_c1_filter::finish_midlife_nav_c1_filter_from_bytes(inputs, checkpoint)
}

/// Integration-only alias for the production monthly byte checkpoint.
pub fn start_midlife_nav_monthly_debug_into(
    inputs: &MidlifeInputs,
    stop_coupon_index: u8,
    checkpoint_out: &mut [u8],
) -> Result<(), MidlifePricerError> {
    crate::worst_of_c1_filter::start_midlife_nav_c1_filter_into(
        inputs,
        stop_coupon_index,
        checkpoint_out,
    )
}

/// Integration-only monthly debug checkpoint advance.
pub fn advance_midlife_nav_monthly_debug_in_place(
    inputs: &MidlifeInputs,
    checkpoint: &mut [u8],
    stop_coupon_index: u8,
) -> Result<(), MidlifePricerError> {
    crate::worst_of_c1_filter::advance_midlife_nav_c1_filter_in_place(
        inputs,
        checkpoint,
        stop_coupon_index,
    )
}

/// Integration-only monthly debug checkpoint finish.
pub fn finish_midlife_nav_monthly_debug_from_bytes(
    inputs: &MidlifeInputs,
    checkpoint: &[u8],
) -> Result<MidlifeNav, MidlifePricerError> {
    crate::worst_of_c1_filter::finish_midlife_nav_c1_filter_from_bytes(inputs, checkpoint)
}

pub fn checkpoint_next_coupon_index(checkpoint: &[u8]) -> Result<u8, MidlifePricerError> {
    crate::worst_of_c1_filter::midlife_checkpoint_next_coupon_index(checkpoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_inputs() -> MidlifeInputs {
        MidlifeInputs {
            current_spy_s6: 0,
            current_qqq_s6: 0,
            current_iwm_s6: 0,
            sigma_common_s6: 0,
            entry_spy_s6: 0,
            entry_qqq_s6: 0,
            entry_iwm_s6: 0,
            beta_spy_s12: 0,
            beta_qqq_s12: 0,
            alpha_s12: 0,
            regression_residual_vol_s6: 0,
            monthly_coupon_schedule: [0; 18],
            quarterly_autocall_schedule: [0; 6],
            next_coupon_index: 0,
            next_autocall_index: 0,
            offered_coupon_bps_s6: 0,
            coupon_barrier_bps: 0,
            autocall_barrier_bps: 0,
            ki_barrier_bps: 0,
            ki_latched: false,
            missed_coupon_observations: 0,
            coupons_paid_usdc: 0,
            notional_usdc: 0,
            now_trading_day: 0,
        }
    }

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
            next_coupon_index: 17,
            next_autocall_index: 5,
            offered_coupon_bps_s6: 500_000_000,
            coupon_barrier_bps: 10_000,
            autocall_barrier_bps: 10_000,
            ki_barrier_bps: 8_000,
            ki_latched: false,
            missed_coupon_observations: 0,
            coupons_paid_usdc: 0,
            notional_usdc: 100_000_000,
            now_trading_day: 378,
        }
    }

    #[test]
    fn final_day_without_ki_returns_par() {
        let mut inputs = sample_inputs();
        inputs.current_spy_s6 = 95_000_000;
        inputs.current_qqq_s6 = 94_000_000;
        inputs.current_iwm_s6 = 93_000_000;

        let nav = compute_midlife_nav(&inputs).expect("final-day nav");
        assert_eq!(nav.nav_s6, 1_000_000);
        assert_eq!(nav.remaining_coupon_pv_s6, 0);
        assert_eq!(nav.par_recovery_probability_s6, 1_000_000);
    }

    #[test]
    fn final_day_knocked_note_tracks_worst_ratio() {
        let mut inputs = sample_inputs();
        inputs.current_spy_s6 = 52_000_000;
        inputs.current_qqq_s6 = 50_000_000;
        inputs.current_iwm_s6 = 51_000_000;
        inputs.ki_latched = true;

        let nav = compute_midlife_nav(&inputs).expect("knocked nav");
        // Tolerance of 1 s6 unit (= 10^-4 bps) matches midlife_reference.rs
        // — systematic floor rounding in the worst_ratio × weight / S6
        // product hits 499_999 on both implementations.
        assert!((nav.nav_s6 - 500_000).abs() <= 1);
        assert_eq!(nav.remaining_coupon_pv_s6, 0);
        assert_eq!(nav.par_recovery_probability_s6, 0);
        assert_eq!(nav.ki_level_usd_s6, 800_000);
    }

    #[test]
    fn zero_state_inputs_are_rejected() {
        let inputs = zero_inputs();
        assert_eq!(
            compute_midlife_nav(&inputs),
            Err(MidlifePricerError::InvalidInput)
        );
    }

    #[test]
    fn checkpointed_midlife_nav_matches_one_shot() {
        let mut inputs = sample_inputs();
        inputs.next_coupon_index = 15;
        inputs.next_autocall_index = 5;
        inputs.now_trading_day = 315;
        inputs.current_spy_s6 = 101_000_000;
        inputs.current_qqq_s6 = 99_000_000;
        inputs.current_iwm_s6 = 98_000_000;

        let one_shot =
            compute_midlife_nav_monthly_debug(&inputs).expect("monthly debug one-shot nav");
        let checkpoint = start_midlife_nav(&inputs, 16).expect("prefix checkpoint");
        assert_eq!(checkpoint.next_coupon_index, 16);
        let resumed = finish_midlife_nav(&inputs, &checkpoint).expect("resumed nav");
        assert_eq!(resumed, one_shot);

        let empty_checkpoint = start_midlife_nav(&inputs, 15).expect("empty checkpoint");
        let advanced =
            advance_midlife_nav(&inputs, &empty_checkpoint, 16).expect("advanced checkpoint");
        let advanced_resumed = finish_midlife_nav(&inputs, &advanced).expect("advanced nav");
        assert_eq!(advanced_resumed, one_shot);

        let mut checkpoint_bytes = vec![0u8; MIDLIFE_CHECKPOINT_BYTES];
        start_midlife_nav_into(&inputs, 16, &mut checkpoint_bytes).expect("byte checkpoint");
        assert!(
            checkpoint_next_coupon_index(&checkpoint_bytes).expect("byte checkpoint cursor") >= 16
        );
        let byte_resumed =
            finish_midlife_nav_from_bytes(&inputs, &checkpoint_bytes).expect("byte resumed nav");

        let mut empty_checkpoint_bytes = vec![0u8; MIDLIFE_CHECKPOINT_BYTES];
        start_midlife_nav_into(&inputs, 15, &mut empty_checkpoint_bytes)
            .expect("empty byte checkpoint");
        advance_midlife_nav_in_place(&inputs, &mut empty_checkpoint_bytes, 16)
            .expect("advanced byte checkpoint");
        let byte_advanced_resumed = finish_midlife_nav_from_bytes(&inputs, &empty_checkpoint_bytes)
            .expect("advanced byte nav");
        let production_one_shot = compute_midlife_nav(&inputs).expect("production one-shot nav");
        assert_eq!(byte_resumed, production_one_shot);
        assert_eq!(byte_advanced_resumed, production_one_shot);
    }

    #[test]
    fn production_byte_checkpoint_preserves_sparse_frontier() {
        let mut inputs = sample_inputs();
        inputs.next_coupon_index = 0;
        inputs.next_autocall_index = 0;
        inputs.now_trading_day = 0;
        inputs.current_spy_s6 = 100_000_000;
        inputs.current_qqq_s6 = 100_000_000;
        inputs.current_iwm_s6 = 100_000_000;
        inputs.sigma_common_s6 = 400_000;

        let production_one_shot = compute_midlife_nav(&inputs).expect("production one-shot nav");
        let mut checkpoint_bytes = vec![0u8; MIDLIFE_CHECKPOINT_BYTES];
        start_midlife_nav_into(&inputs, 9, &mut checkpoint_bytes).expect("front checkpoint");
        assert_eq!(
            checkpoint_next_coupon_index(&checkpoint_bytes).expect("front checkpoint cursor"),
            9
        );
        let resumed =
            finish_midlife_nav_from_bytes(&inputs, &checkpoint_bytes).expect("resumed nav");
        assert_eq!(resumed, production_one_shot);
    }

    #[test]
    fn inputs_struct_is_copy_and_serializable() {
        let inputs = zero_inputs();
        let copied = inputs;
        let json = serde_json::to_string(&copied).expect("serialize MidlifeInputs");
        let roundtrip: MidlifeInputs =
            serde_json::from_str(&json).expect("deserialize MidlifeInputs");
        assert_eq!(roundtrip, inputs);
    }

    #[test]
    fn nav_struct_is_copy_and_serializable() {
        let nav = MidlifeNav {
            nav_s6: 1_000_000,
            ki_level_usd_s6: 600_000,
            remaining_coupon_pv_s6: 50_000,
            par_recovery_probability_s6: 750_000,
        };
        let copied = nav;
        let json = serde_json::to_string(&copied).expect("serialize MidlifeNav");
        let roundtrip: MidlifeNav = serde_json::from_str(&json).expect("deserialize MidlifeNav");
        assert_eq!(roundtrip, nav);
    }
}
