//! 2-token IL threshold computation.
//!
//! Thin wrapper over solmath_core::il_thresholds. Accepts u64 weights/targets
//! (matching halcyon-common conventions) and delegates to the solmath u128 API.

use solmath_core::SolMathError;

/// Compute impermanent loss for a 2-token pool at price ratio `x`.
///
/// IL(x) = w·x + (1-w) - x^w
///
/// * `weight` — weight of first token at SCALE (u64)
/// * `x` — price ratio P_current / P_entry at SCALE (u128)
///
/// Returns IL at SCALE.
pub fn compute_il(weight: u64, x: u128) -> Result<u128, SolMathError> {
    solmath_core::compute_il(weight as u128, x)
}

/// Find price ratio thresholds where IL = `il_target` for a 2-token pool.
///
/// Returns `(x_lower, x_upper)` at SCALE.
/// `x_lower = 0` (sentinel) when il_target ≥ 1 - weight (one-sided).
///
/// * `weight` — weight of first token at SCALE (u64)
/// * `il_target` — IL level to solve for at SCALE (u64, typically = cap)
pub fn compute_il_thresholds(weight: u64, il_target: u64) -> Result<(u128, u128), SolMathError> {
    solmath_core::il_thresholds(weight as u128, il_target as u128)
}

/// Verify that IL at price ratio `x` is within `tolerance` of target.
///
/// Single pow_fixed call (~14K CU). For on-chain verification of
/// off-chain computed thresholds.
pub fn verify_il_at_threshold(
    weight: u64,
    x: u128,
    il_target: u64,
    tolerance: u64,
) -> Result<bool, SolMathError> {
    solmath_core::verify_il_at_threshold(weight as u128, x, il_target as u128, tolerance as u128)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solmath_core::SCALE;

    const SCALE_U64: u64 = SCALE as u64;
    const HALF: u64 = SCALE_U64 / 2;

    #[test]
    fn wrapper_round_trip_50_50() {
        let cap: u64 = 250_000_000_000; // 0.25
        let (x_lo, x_up) = compute_il_thresholds(HALF, cap).unwrap();

        let il_up = compute_il(HALF, x_up).unwrap();
        let il_lo = compute_il(HALF, x_lo).unwrap();

        let diff_up = (il_up as i128 - cap as i128).unsigned_abs();
        let diff_lo = (il_lo as i128 - cap as i128).unsigned_abs();

        assert!(diff_up < 10_000, "upper: diff={diff_up}");
        assert!(diff_lo < 10_000, "lower: diff={diff_lo}");
    }

    #[test]
    fn wrapper_round_trip_70_30() {
        let w: u64 = 700_000_000_000;
        let cap: u64 = 200_000_000_000;
        let (x_lo, x_up) = compute_il_thresholds(w, cap).unwrap();

        assert!(x_lo > 0 && (x_lo as u128) < SCALE);
        assert!((x_up as u128) > SCALE);

        assert!(verify_il_at_threshold(w, x_up, cap, 1_000).unwrap());
        assert!(verify_il_at_threshold(w, x_lo, cap, 1_000).unwrap());
    }

    #[test]
    fn wrapper_one_sided() {
        let w: u64 = 200_000_000_000; // 0.2
        let h: u64 = 850_000_000_000; // 0.85 ≥ 1-w = 0.8
        let (x_lo, _) = compute_il_thresholds(w, h).unwrap();
        assert_eq!(x_lo, 0);
    }
}
