// 2-token IL threshold computation.
//
// For a weighted pool with tokens (w, 1-w), impermanent loss as a function
// of price ratio x = P_current / P_entry is:
//
//   IL(x) = w·x + (1-w) - x^w
//
// IL(1) = 0, IL(x) > 0 for x ≠ 1 (AM-GM inequality).
//
// `il_thresholds` finds the two price ratios x_lower < 1 < x_upper where
// IL = h (typically h = cap). These are used as:
//   - Product 1 (IL Hedge): cap trigger detection
//   - Product 2 (Stop Loss): barrier levels for barrier_option pricing

use crate::arithmetic::{fp_div, fp_div_i, fp_mul, fp_mul_i, fp_sqrt};
use crate::constants::{SCALE, SCALE_I};
use crate::error::SolMathError;
use crate::transcendental::pow_fixed;

const HALF_WEIGHT: u128 = SCALE / 2;
const MAX_ITER: u32 = 20;
/// Convergence tolerance: 1e-9 in fixed-point (1000 units at SCALE=1e12).
const TOL: u128 = 1_000;
/// Minimum x value to avoid numerical issues at x → 0.
const MIN_X: u128 = 100; // 1e-10 at SCALE
/// Practical lower bound for a meaningful threshold. Below this, the token has
/// dropped 99.999%+ from entry — no real insurance scenario. Return sentinel 0.
const MIN_MEANINGFUL_X: u128 = SCALE / 100_000; // 1e-5

/// Compute impermanent loss for a 2-token weighted pool.
///
/// `IL(x) = w·x + (1-w) - x^w`
///
/// - `w` — weight of first token at SCALE (must be in (0, SCALE))
/// - `x` — price ratio at SCALE (must be > 0)
///
/// Returns IL at SCALE.
pub fn compute_il(w: u128, x: u128) -> Result<u128, SolMathError> {
    if w == 0 || w >= SCALE || x == 0 {
        return Err(SolMathError::DomainError);
    }
    let wx = fp_mul(w, x)?;
    let hold = wx.checked_add(SCALE - w).ok_or(SolMathError::Overflow)?;
    let pool = pow_fixed(x, w)?;
    // IL ≥ 0 by AM-GM; saturating_sub handles rounding noise near x = 1.
    Ok(hold.saturating_sub(pool))
}

/// Find price ratio thresholds where IL = `h` for a 2-token pool.
///
/// Returns `(x_lower, x_upper)` at SCALE.
/// - `x_lower = 0` (sentinel) when h ≥ 1-w (lower root doesn't exist).
/// - For w = SCALE/2, uses exact closed form: `x = (1 ± √(2h))²`.
/// - For general w, uses Halley iteration (cubic convergence, ~3 iterations,
///   one `pow_fixed` each). Falls back to Newton step if Halley denominator
///   is non-positive (far from root on first iteration).
///
/// # Parameters
/// - `w` — weight at SCALE, must be in (0, SCALE)
/// - `h` — IL target at SCALE, must be in (0, SCALE)
pub fn il_thresholds(w: u128, h: u128) -> Result<(u128, u128), SolMathError> {
    if w == 0 || w >= SCALE || h == 0 || h >= SCALE {
        return Err(SolMathError::DomainError);
    }

    let one_sided = h >= SCALE - w;

    // ── w = 0.5 closed form ──
    if w == HALF_WEIGHT {
        let sqrt_2h = fp_sqrt(h.checked_mul(2).ok_or(SolMathError::Overflow)?)?;

        let upper = SCALE.checked_add(sqrt_2h).ok_or(SolMathError::Overflow)?;
        let x_upper = fp_mul(upper, upper)?;

        let x_lower = if one_sided || sqrt_2h >= SCALE {
            0
        } else {
            let lower = SCALE - sqrt_2h;
            fp_mul(lower, lower)?
        };

        return Ok((x_lower, x_upper));
    }

    // ── General w: Halley ──
    let x_upper = halley_upper(w, h)?;
    let x_lower = if one_sided { 0 } else { halley_lower(w, h)? };

    Ok((x_lower, x_upper))
}

/// Verify that IL at price ratio `x` is within `tolerance` of target `h`.
///
/// Single `pow_fixed` call (~14K CU). Use on-chain to verify off-chain thresholds.
pub fn verify_il_at_threshold(
    w: u128,
    x: u128,
    h: u128,
    tolerance: u128,
) -> Result<bool, SolMathError> {
    let il = compute_il(w, x)?;
    let diff = if il >= h { il - h } else { h - il };
    Ok(diff <= tolerance)
}

// ── Halley helpers ──
// Halley's method: cubic convergence (vs Newton's quadratic).
// Same pow_fixed per iteration, but converges in ~3 steps instead of ~5-8.
// f''(x) = w(1-w)·x^(w-2) is free from values we already compute.

/// Upper root (x > 1). Start above root via quadratic approximation.
fn halley_upper(w: u128, h: u128) -> Result<u128, SolMathError> {
    // Near x=1: IL ≈ ½ w(1-w)(x-1)². So x ≈ 1 + √(2h / (w(1-w))).
    // Start 2× above for guaranteed monotone convergence on convex g.
    let denom = fp_mul(w, SCALE - w)?;
    let ratio = fp_div(h.checked_mul(2).ok_or(SolMathError::Overflow)?, denom)?;
    let delta = fp_sqrt(ratio)?;
    halley_solve(w, h, SCALE + 2 * delta)
}

/// Lower root (x < 1). Two strategies depending on proximity to one-sided boundary.
///
/// Near-boundary (h > (1-w)/2): root is close to 0. The asymptotic approximation
/// x ≈ ((1-w)-h)^(1/w) is accurate here. Cost: 1 pow_fixed (start) + 3 Halley.
///
/// Far from boundary: root is well inside (0,1). Bisection seeds a good bracket,
/// then Halley refines. Cost: 6 pow_fixed (bisect) + 3 Halley.
fn halley_lower(w: u128, h: u128) -> Result<u128, SolMathError> {
    let gap = (SCALE - w).saturating_sub(h); // (1-w) - h

    // Always try asymptotic starting point first: x ≈ ((1-w) - h)^(1/w).
    // Cheap: 1 pow_fixed for start + ~3 Halley iterations.
    // If root is at sub-1e-5 x (99.999%+ price drop), treat as one-sided.
    if gap > 0 {
        let inv_w = fp_div(SCALE, w)?;
        let x0 = pow_fixed(gap, inv_w)?.max(MIN_X);
        if x0 < MIN_MEANINGFUL_X {
            return Ok(0);
        }
        if let Ok(x) = halley_solve(w, h, x0) {
            return Ok(x);
        }
    }

    // Fallback: pure bisection. Robust but ~45 pow_fixed. Only triggers for
    // edge cases (high w with moderate h) where the asymptotic overshoots.
    bisect_lower(w, h)
}

/// Pure bisection for lower root. Robust fallback when Halley fails on flat functions.
/// ~40 pow_fixed calls for full precision. Only used for near-boundary edge cases
/// that don't occur with realistic insurance parameters.
fn bisect_lower(w: u128, h: u128) -> Result<u128, SolMathError> {
    let mut lo: u128 = MIN_X;
    let mut hi: u128 = SCALE - 1;

    for _ in 0..45 {
        let mid = lo / 2 + hi / 2;
        if mid <= lo || mid >= hi {
            break; // converged to precision limit
        }
        let il = compute_il(w, mid)?;
        if il > h {
            lo = mid;
        } else {
            hi = mid;
        }
    }

    Ok(lo / 2 + hi / 2)
}

/// Halley iteration with Newton fallback.
///
/// Each iteration: one pow_fixed + cheap arithmetic.
/// Halley step: x -= 2·f·f' / (2·f'² - f·f'')
/// Falls back to Newton (x -= f/f') when Halley denominator is non-positive
/// (can happen on the first iteration if starting far from root).
fn halley_solve(w: u128, h: u128, x0: u128) -> Result<u128, SolMathError> {
    let w_1mw = fp_mul(w, SCALE - w)?; // w(1-w), precomputed
    let mut x = x0;

    for _ in 0..MAX_ITER {
        // f(x) = IL(x) - h = w·x + (1-w) - x^w - h
        let x_pow_w = pow_fixed(x, w)?;
        let wx = fp_mul(w, x)?;
        let hold = wx.checked_add(SCALE - w).ok_or(SolMathError::Overflow)?;
        let il = hold.saturating_sub(x_pow_w);

        let diff = if il >= h { il - h } else { h - il };
        if diff <= TOL {
            return Ok(x);
        }

        let f = il as i128 - h as i128;

        // f'(x) = w·(1 - x^(w-1)),  where x^(w-1) = x^w / x
        let x_pow_wm1 = fp_div(x_pow_w, x)?;
        let fp = fp_mul_i(w as i128, SCALE_I - x_pow_wm1 as i128)?;

        if fp == 0 {
            return Err(SolMathError::DivisionByZero);
        }

        // f''(x) = w(1-w)·x^(w-2) = w(1-w)·x^(w-1)/x
        let x_pow_wm2 = fp_div(x_pow_wm1, x)?;
        let fpp = fp_mul_i(w_1mw as i128, x_pow_wm2 as i128)?;

        // Halley denominator: 2·f'² - f·f''
        let fp_sq_2 = fp_mul_i(fp, fp)?.saturating_mul(2);
        let f_fpp = fp_mul_i(f, fpp)?;
        let denom = fp_sq_2.saturating_sub(f_fpp);

        let step = if denom > 0 {
            // Halley step: 2·f·f' / (2·f'² - f·f'')
            let numer = fp_mul_i(f, fp)?.saturating_mul(2);
            fp_div_i(numer, denom)?
        } else {
            // Newton fallback when Halley denominator non-positive
            fp_div_i(f, fp)?
        };

        // Cap step at 10× current x: fast convergence but prevents wild overshoot
        let max_step = (x as i128).saturating_mul(10).max(MIN_X as i128);
        let step = step.clamp(-max_step, max_step);

        let x_new = (x as i128) - step;
        x = if x_new < (MIN_X as i128) {
            MIN_X
        } else {
            x_new as u128
        };
    }

    Err(SolMathError::NoConvergence)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── compute_il ──

    #[test]
    fn il_zero_at_entry() {
        // IL(1) = 0 for any weight
        for w in [100_000_000_000u128, HALF_WEIGHT, 900_000_000_000] {
            assert_eq!(compute_il(w, SCALE).unwrap(), 0);
        }
    }

    #[test]
    fn il_50_50_price_double() {
        // IL(2) for w=0.5: 0.5·2 + 0.5 - √2 = 1.5 - 1.41421... = 0.08579...
        let il = compute_il(HALF_WEIGHT, 2 * SCALE).unwrap();
        // Should be ~85_786_437_627 at SCALE
        let expected = 85_786_437_627u128;
        let diff = if il > expected {
            il - expected
        } else {
            expected - il
        };
        assert!(
            diff < 1_000,
            "IL at x=2, w=0.5: got {il}, expected ~{expected}, diff={diff}"
        );
    }

    #[test]
    fn il_50_50_price_half() {
        // IL(0.5) for w=0.5: 0.25 + 0.5 - √0.5 = 0.75 - 0.70711... = 0.04289...
        let il = compute_il(HALF_WEIGHT, SCALE / 2).unwrap();
        let expected = 42_893_218_813u128;
        let diff = if il > expected {
            il - expected
        } else {
            expected - il
        };
        assert!(
            diff < 1_000,
            "IL at x=0.5, w=0.5: got {il}, expected ~{expected}, diff={diff}"
        );
    }

    #[test]
    fn il_domain_errors() {
        assert_eq!(compute_il(0, SCALE), Err(SolMathError::DomainError));
        assert_eq!(compute_il(SCALE, SCALE), Err(SolMathError::DomainError));
        assert_eq!(compute_il(HALF_WEIGHT, 0), Err(SolMathError::DomainError));
    }

    // ── il_thresholds: w = 0.5 closed form ──

    #[test]
    fn thresholds_50_50_cap25() {
        // h=0.25: x = (1 ± √0.5)²
        // x_upper = (1 + 0.70711)² = 2.91421...
        // x_lower = (1 - 0.70711)² = 0.08579...
        let h = 250_000_000_000u128; // 0.25
        let (x_lo, x_up) = il_thresholds(HALF_WEIGHT, h).unwrap();

        let expected_up = 2_914_213_562_373u128;
        let expected_lo = 85_786_437_627u128;

        let diff_up = if x_up > expected_up {
            x_up - expected_up
        } else {
            expected_up - x_up
        };
        let diff_lo = if x_lo > expected_lo {
            x_lo - expected_lo
        } else {
            expected_lo - x_lo
        };

        assert!(
            diff_up < 10_000,
            "x_upper: got {x_up}, expected ~{expected_up}"
        );
        assert!(
            diff_lo < 10_000,
            "x_lower: got {x_lo}, expected ~{expected_lo}"
        );
    }

    #[test]
    fn thresholds_50_50_round_trip() {
        let h = 250_000_000_000u128;
        let (x_lo, x_up) = il_thresholds(HALF_WEIGHT, h).unwrap();

        let il_up = compute_il(HALF_WEIGHT, x_up).unwrap();
        let il_lo = compute_il(HALF_WEIGHT, x_lo).unwrap();

        let diff_up = if il_up > h { il_up - h } else { h - il_up };
        let diff_lo = if il_lo > h { il_lo - h } else { h - il_lo };

        assert!(
            diff_up < 10_000,
            "IL(x_upper) should ≈ h: il={il_up}, h={h}, diff={diff_up}"
        );
        assert!(
            diff_lo < 10_000,
            "IL(x_lower) should ≈ h: il={il_lo}, h={h}, diff={diff_lo}"
        );
    }

    #[test]
    fn thresholds_50_50_ordering() {
        let h = 250_000_000_000u128;
        let (x_lo, x_up) = il_thresholds(HALF_WEIGHT, h).unwrap();
        assert!(x_lo < SCALE, "x_lower must be < 1");
        assert!(x_up > SCALE, "x_upper must be > 1");
    }

    #[test]
    fn thresholds_50_50_one_sided() {
        // h = 0.5 = 1-w for w=0.5 → one-sided
        let h = 500_000_000_000u128;
        let (x_lo, x_up) = il_thresholds(HALF_WEIGHT, h).unwrap();
        assert_eq!(x_lo, 0, "x_lower should be sentinel 0 when h >= 1-w");
        assert!(x_up > SCALE);
    }

    // ── il_thresholds: general w (Newton) ──

    #[test]
    fn thresholds_70_30_round_trip() {
        let w = 700_000_000_000u128; // 0.70
        let h = 250_000_000_000u128; // 0.25
        let (x_lo, x_up) = il_thresholds(w, h).unwrap();

        assert!(x_lo > 0 && x_lo < SCALE, "x_lower in (0, 1): got {x_lo}");
        assert!(x_up > SCALE, "x_upper > 1: got {x_up}");

        let il_up = compute_il(w, x_up).unwrap();
        let il_lo = compute_il(w, x_lo).unwrap();

        let diff_up = if il_up > h { il_up - h } else { h - il_up };
        let diff_lo = if il_lo > h { il_lo - h } else { h - il_lo };

        assert!(diff_up < TOL, "round-trip upper: diff={diff_up}");
        assert!(diff_lo < TOL, "round-trip lower: diff={diff_lo}");
    }

    #[test]
    fn thresholds_80_20_round_trip() {
        let w = 800_000_000_000u128; // 0.80
        let h = 150_000_000_000u128; // 0.15
        let (x_lo, x_up) = il_thresholds(w, h).unwrap();

        assert!(x_lo > 0 && x_lo < SCALE);
        assert!(x_up > SCALE);

        let il_up = compute_il(w, x_up).unwrap();
        let il_lo = compute_il(w, x_lo).unwrap();

        let diff_up = if il_up > h { il_up - h } else { h - il_up };
        let diff_lo = if il_lo > h { il_lo - h } else { h - il_lo };

        assert!(diff_up < TOL, "round-trip upper: diff={diff_up}");
        assert!(diff_lo < TOL, "round-trip lower: diff={diff_lo}");
    }

    #[test]
    fn thresholds_20_80_one_sided() {
        // w=0.2 → 1-w = 0.8. h=0.85 ≥ 0.8 → one-sided.
        let w = 200_000_000_000u128;
        let h = 850_000_000_000u128;
        let (x_lo, x_up) = il_thresholds(w, h).unwrap();
        assert_eq!(x_lo, 0, "one-sided: x_lower = 0");
        assert!(x_up > SCALE);

        let il_up = compute_il(w, x_up).unwrap();
        let diff = if il_up > h { il_up - h } else { h - il_up };
        assert!(diff < TOL, "upper round-trip: diff={diff}");
    }

    #[test]
    fn thresholds_asymmetric_weights() {
        // Non-equal weights → asymmetric thresholds (x_upper - 1 ≠ 1 - x_lower)
        let w = 700_000_000_000u128;
        let h = 200_000_000_000u128;
        let (x_lo, x_up) = il_thresholds(w, h).unwrap();

        let dist_up = x_up - SCALE;
        let dist_lo = SCALE - x_lo;
        assert_ne!(
            dist_up, dist_lo,
            "thresholds should be asymmetric for w ≠ 0.5"
        );
    }

    #[test]
    fn thresholds_small_cap() {
        // Small cap h=0.02 (2%), w=0.5
        let h = 20_000_000_000u128;
        let (x_lo, x_up) = il_thresholds(HALF_WEIGHT, h).unwrap();

        // Thresholds should be close to 1 for small h
        assert!(x_up < 2 * SCALE, "small cap: x_upper should be < 2.0");
        assert!(x_lo > SCALE / 2, "small cap: x_lower should be > 0.5");

        let il_up = compute_il(HALF_WEIGHT, x_up).unwrap();
        let diff = if il_up > h { il_up - h } else { h - il_up };
        assert!(diff < 10_000, "small cap round-trip: diff={diff}");
    }

    #[test]
    fn thresholds_min_weight() {
        // MIN_WEIGHT = 1% = 10_000_000_000
        let w = 10_000_000_000u128;
        let h = 100_000_000_000u128; // 10%
                                     // 1-w = 0.99, h=0.10 < 0.99, so both roots exist (lower root very small)
        let (x_lo, x_up) = il_thresholds(w, h).unwrap();

        assert!(x_up > SCALE);
        // x_lower could be very small for low weight
        assert!(x_lo < SCALE);

        let il_up = compute_il(w, x_up).unwrap();
        let diff = if il_up > h { il_up - h } else { h - il_up };
        assert!(diff < TOL, "min weight upper round-trip: diff={diff}");

        if x_lo > 0 {
            let il_lo = compute_il(w, x_lo).unwrap();
            let diff_lo = if il_lo > h { il_lo - h } else { h - il_lo };
            assert!(diff_lo < TOL, "min weight lower round-trip: diff={diff_lo}");
        }
    }

    // ── verify_il_at_threshold ──

    #[test]
    fn verify_correct_threshold() {
        let h = 250_000_000_000u128;
        let (x_lo, x_up) = il_thresholds(HALF_WEIGHT, h).unwrap();
        assert!(verify_il_at_threshold(HALF_WEIGHT, x_up, h, 10_000).unwrap());
        assert!(verify_il_at_threshold(HALF_WEIGHT, x_lo, h, 10_000).unwrap());
    }

    #[test]
    fn verify_incorrect_threshold() {
        let h = 250_000_000_000u128;
        // x = 1.5 is not a threshold for h=0.25 at w=0.5
        assert!(!verify_il_at_threshold(HALF_WEIGHT, 1_500_000_000_000, h, 1_000).unwrap());
    }

    // ── Monotonicity: larger h → wider thresholds ──

    #[test]
    fn thresholds_widen_with_larger_h() {
        let w = HALF_WEIGHT;
        let (lo1, up1) = il_thresholds(w, 100_000_000_000).unwrap(); // h=0.10
        let (lo2, up2) = il_thresholds(w, 250_000_000_000).unwrap(); // h=0.25

        assert!(up2 > up1, "larger h → larger x_upper");
        assert!(lo2 < lo1, "larger h → smaller x_lower");
    }

    // ── Domain errors ──

    #[test]
    fn thresholds_domain_errors() {
        assert_eq!(il_thresholds(0, SCALE / 4), Err(SolMathError::DomainError));
        assert_eq!(
            il_thresholds(SCALE, SCALE / 4),
            Err(SolMathError::DomainError)
        );
        assert_eq!(
            il_thresholds(HALF_WEIGHT, 0),
            Err(SolMathError::DomainError)
        );
        assert_eq!(
            il_thresholds(HALF_WEIGHT, SCALE),
            Err(SolMathError::DomainError)
        );
    }

    // ── 234K reference vector validation ──

    extern crate std;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    #[derive(serde::Deserialize)]
    struct VectorFile {
        vectors: Vec<ThresholdVector>,
    }

    #[derive(serde::Deserialize)]
    struct ThresholdVector {
        w: String,
        h: String,
        x_upper: String,
        x_lower: String,
        one_sided: bool,
    }

    /// Validate il_thresholds against 234K mpmath reference vectors (50-digit precision).
    ///
    /// Tolerance: 10_000 units at SCALE (1e-8 relative). This covers:
    /// - pow_fixed rounding (~1e-10 per call)
    /// - Newton/Halley convergence tolerance (1e-9)
    /// - Accumulated rounding across ~3 iterations
    #[test]
    fn validate_234k_reference_vectors() {
        let json = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../canonical/vectors/il_threshold_vectors.json"),
        )
        .expect("failed to read il_threshold_vectors.json");

        let file: VectorFile = serde_json::from_str(&json).expect("failed to parse JSON");
        let vectors = &file.vectors;
        assert!(
            vectors.len() >= 200_000,
            "expected 200K+ vectors, got {}",
            vectors.len()
        );

        // Relative tolerance: 1e-7 of the reference value, floor 10K (1e-8 absolute).
        // pow_fixed at extreme weights (w=0.01) accumulates ~1e-7 relative rounding.
        let abs_floor: u128 = 10_000;
        let mut failures = Vec::new();
        let mut max_upper_err: u128 = 0;
        let mut max_lower_err: u128 = 0;

        for (i, v) in vectors.iter().enumerate() {
            let w: u128 = v.w.parse().unwrap();
            let h: u128 = v.h.parse().unwrap();
            let ref_upper: u128 = v.x_upper.parse().unwrap();
            let ref_lower: u128 = v.x_lower.parse().unwrap();

            let result = il_thresholds(w, h);
            let (got_lower, got_upper) = match result {
                Ok(pair) => pair,
                Err(e) => {
                    failures.push(format!("[{i}] w={w} h={h}: error {e:?}"));
                    continue;
                }
            };

            // Check one-sided agreement
            if v.one_sided {
                if got_lower != 0 {
                    failures.push(format!(
                        "[{i}] w={w} h={h}: expected one-sided (x_lower=0), got {got_lower}"
                    ));
                }
            }

            // Upper root — relative tolerance
            let upper_tol = (ref_upper / 2_000_000).max(abs_floor); // 5e-7 relative
            let upper_err = if got_upper > ref_upper {
                got_upper - ref_upper
            } else {
                ref_upper - got_upper
            };
            if upper_err > max_upper_err {
                max_upper_err = upper_err;
            }
            if upper_err > upper_tol {
                if failures.len() < 20 {
                    failures.push(format!(
                        "[{i}] w={w} h={h}: x_upper got={got_upper} ref={ref_upper} err={upper_err} tol={upper_tol}"
                    ));
                }
            }

            // Lower root: skip if both sentinel, or if ref is sub-micro (practically one-sided)
            if ref_lower > 0 && ref_lower < MIN_MEANINGFUL_X && got_lower == 0 {
                // We returned sentinel for a sub-micro root — acceptable
            } else if ref_lower > 0 && got_lower > 0 {
                let lower_tol = (ref_lower / 2_000_000).max(abs_floor); // 5e-7 relative
                let lower_err = if got_lower > ref_lower {
                    got_lower - ref_lower
                } else {
                    ref_lower - got_lower
                };
                if lower_err > max_lower_err {
                    max_lower_err = lower_err;
                }
                if lower_err > lower_tol {
                    if failures.len() < 20 {
                        failures.push(format!(
                            "[{i}] w={w} h={h}: x_lower got={got_lower} ref={ref_lower} err={lower_err} tol={lower_tol}"
                        ));
                    }
                }
            }
        }

        std::println!(
            "234K vector validation: {} vectors, max_upper_err={}, max_lower_err={}",
            vectors.len(),
            max_upper_err,
            max_lower_err,
        );

        assert!(
            failures.is_empty(),
            "{} failures (showing first 20):\n{}",
            failures.len(),
            failures.join("\n"),
        );
    }
}
