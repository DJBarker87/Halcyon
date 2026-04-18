use crate::arithmetic::{fp_div_i_round, fp_mul_i_round};
use crate::constants::*;
use crate::error::SolMathError;
use crate::hp::{
    downscale_hp_to_std_i, exp_fixed_hp, fp_div_hp_safe, fp_mul_hp_i, ln_fixed_hp,
    upscale_std_to_hp,
};
use crate::transcendental::ln_fixed_i;

// ============================================================
// ln_gamma Remez polynomial coefficients (mpmath 50-digit precision)
// ============================================================

// Interval A: [2, 5], degree 17.  t = (a - 3.5) / 1.5, t ∈ [-1, 1].
// Max approximation error: 1.55 ULP.
const LNG_A: [i128; 18] = [
    1_200_973_602_347, // c0
    1_654_734_960_968, // c1
    371_652_475_576,   // c2
    -60_864_779_034,   // c3
    14_830_140_955,    // c4
    -4_302_872_239,    // c5
    1_377_135_348,     // c6
    -469_039_237,      // c7
    166_728_022,       // c8
    -61_082_260,       // c9
    22_742_281,        // c10
    -8_668_423,        // c11
    3_602_195,         // c12
    -1_411_345,        // c13
    307_834,           // c14
    -120_254,          // c15
    180_616,           // c16
    -73_003,           // c17
];
// Midpoint and half-width for interval A (as fixed-point)
const LNG_A_MID: i128 = 3_500_000_000_000; // 3.5 * SCALE
const LNG_A_HW: i128 = 1_500_000_000_000; // 1.5 * SCALE

// Interval B: [5, 10], degree 14.  t = (a - 7.5) / 2.5, t ∈ [-1, 1].
// Max approximation error: 1.11 ULP.
const LNG_B: [i128; 15] = [
    7_534_364_236_759, // c0
    4_866_893_710_608, // c1
    445_674_677_179,   // c2
    -52_878_261_542,   // c3
    9_395_241_870,     // c4
    -1_999_907_700,    // c5
    472_246_061,       // c6
    -119_274_944,      // c7
    31_585_278,        // c8
    -8_694_972,        // c9
    2_449_466,         // c10
    -659_723,          // c11
    192_375,           // c12
    -86_860,           // c13
    26_226,            // c14
];
const LNG_B_MID: i128 = 7_500_000_000_000; // 7.5 * SCALE
const LNG_B_HW: i128 = 2_500_000_000_000; // 2.5 * SCALE

/// Evaluate a polynomial via Horner's method in fixed-point.
/// coeffs[0] + coeffs[1]*t + coeffs[2]*t^2 + ...
/// All values at SCALE. t must be in [-SCALE, SCALE].
#[inline]
fn horner_eval(coeffs: &[i128], t: i128) -> Result<i128, SolMathError> {
    let n = coeffs.len();
    let mut p = coeffs[n - 1];
    for i in (0..n - 1).rev() {
        p = fp_mul_i_round(p, t)? + coeffs[i];
    }
    Ok(p)
}

/// Log of the gamma function: ln(Γ(a/SCALE)) × SCALE.
///
/// Two-interval Remez polynomial (degree 17 on [2,5], degree 14 on [5,10])
/// with recurrence reduction for a < 2·SCALE.
///
/// - **a**: signed fixed-point at `SCALE` (1e12). Must satisfy 0.5 ≤ a/SCALE ≤ 10.
/// - **Returns**: `i128` at `SCALE`.
/// - **Errors**: `DomainError` if a < 0.5·SCALE or a > 10·SCALE.
/// - **Accuracy**: max 4 ULP over [0.5, 10].
pub fn ln_gamma(a: i128) -> Result<i128, SolMathError> {
    const HALF: i128 = SCALE_I / 2; // 0.5
    const TEN: i128 = 10 * SCALE_I; // 10.0
    const TWO: i128 = 2 * SCALE_I; // 2.0
    const ONE: i128 = SCALE_I; // 1.0
    const FIVE: i128 = 5 * SCALE_I; // 5.0

    if a < HALF || a > TEN {
        return Err(SolMathError::DomainError);
    }

    // Recurrence: shift a into [2, 10] for polynomial evaluation.
    // ln(Γ(a)) = ln(Γ(a+1)) - ln(a)
    let (aa, ln_correction) = if a < ONE {
        // a ∈ [0.5, 1): shift by 2 → a+2 ∈ [2.5, 3)
        // ln(Γ(a)) = ln(Γ(a+2)) - ln(a) - ln(a+1)
        let ln_a = ln_fixed_i(a as u128)?;
        let ln_a1 = ln_fixed_i((a + ONE) as u128)?;
        (a + TWO, ln_a + ln_a1)
    } else if a < TWO {
        // a ∈ [1, 2): shift by 1 → a+1 ∈ [2, 3)
        // ln(Γ(a)) = ln(Γ(a+1)) - ln(a)
        let ln_a = ln_fixed_i(a as u128)?;
        (a + ONE, ln_a)
    } else {
        (a, 0i128)
    };

    // Choose polynomial interval and evaluate.
    // Use rounding division for t to minimize input error propagation.
    let poly_val = if aa <= FIVE {
        // Interval A: [2, 5], t = (a - 3.5) / 1.5
        let t = fp_div_i_round(aa - LNG_A_MID, LNG_A_HW)?;
        horner_eval(&LNG_A, t)?
    } else {
        // Interval B: [5, 10], t = (a - 7.5) / 2.5
        let t = fp_div_i_round(aa - LNG_B_MID, LNG_B_HW)?;
        horner_eval(&LNG_B, t)?
    };

    Ok(poly_val - ln_correction)
}

/// Upper regularized incomplete gamma function: Q(a, x) = Γ(a, x) / Γ(a).
///
/// Uses the series expansion for P(a,x) when x < a+1, returning Q = 1-P.
/// Uses the continued fraction (modified Lentz) for Q(a,x) when x ≥ a+1.
///
/// - **a**: signed fixed-point at `SCALE` (1e12). Must satisfy 0 < a/SCALE ≤ 10.
///   For a < 0.5, uses recurrence Q(a,x) = Q(a+1,x) − x^a·e^{-x}/Γ(a+1).
/// - **x**: signed fixed-point at `SCALE` (1e12). Must satisfy 0 ≤ x/SCALE ≤ 50.
/// - **Returns**: `i128` at `SCALE`, in [0, SCALE].
/// - **Errors**: `DomainError` if a ≤ 0 or x out of range.
/// - **Accuracy**: max 5 ULP.
pub fn regularized_gamma_q(a: i128, x: i128) -> Result<i128, SolMathError> {
    const HALF: i128 = SCALE_I / 2;
    const TEN: i128 = 10 * SCALE_I;
    const FIFTY: i128 = 50 * SCALE_I;

    if a <= 0 || a > TEN {
        return Err(SolMathError::DomainError);
    }
    if x < 0 || x > FIFTY {
        return Err(SolMathError::DomainError);
    }

    // Q(a, 0) = 1
    if x == 0 {
        return Ok(SCALE_I);
    }

    // For a < 0.5: use recurrence to shift into [0.5, 10] domain.
    // Q(a, x) = Q(a+1, x) − x^a · e^{-x} / Γ(a+1)
    if a < HALF {
        let a1 = a + SCALE_I;
        let q_a1 = regularized_gamma_q(a1, x)?;

        // Compute x^a · e^{-x} / Γ(a+1) in log-space via HP
        // log_term = a·ln(x) − x − ln_gamma(a+1)
        let x_hp = upscale_std_to_hp(x as u128)?;
        let a_hp = upscale_std_to_hp(a as u128)?;
        let ln_x_hp = ln_fixed_hp(x_hp)?;
        let a_ln_x_hp = fp_mul_hp_i(a_hp, ln_x_hp)?;
        let lng_a1 = ln_gamma(a1)?;
        let lng_a1_hp = lng_a1 * HP_TO_STD;
        let log_term_hp = a_ln_x_hp - x_hp - lng_a1_hp;

        if log_term_hp < -40 * SCALE_HP {
            // Correction term is negligible
            return Ok(q_a1.clamp(0, SCALE_I));
        }
        let correction_hp = exp_fixed_hp(log_term_hp)?;
        let correction = downscale_hp_to_std_i(correction_hp);

        let q = q_a1 - correction;
        return Ok(q.clamp(0, SCALE_I));
    }

    // Compute log_prefix = -x + a·ln(x) - ln_gamma(a) using HP (1e15) arithmetic
    // to avoid error amplification from a·ln(x) when a is large.
    let x_hp = upscale_std_to_hp(x as u128)?;
    let a_hp = upscale_std_to_hp(a as u128)?;
    let ln_x_hp = ln_fixed_hp(x_hp)?;
    let a_ln_x_hp = fp_mul_hp_i(a_hp, ln_x_hp)?;
    let lng_a = ln_gamma(a)?;
    let lng_a_hp = lng_a * HP_TO_STD; // upscale ln_gamma result to HP
    let log_prefix_hp = -x_hp + a_ln_x_hp - lng_a_hp;

    // If log_prefix is extremely negative, the prefix is essentially 0.
    if log_prefix_hp < -40 * SCALE_HP {
        if x < a + SCALE_I {
            return Ok(SCALE_I); // Series: P ≈ 0, Q ≈ 1
        } else {
            return Ok(0); // CF: Q ≈ 0
        }
    }

    if x >= 20 * SCALE_I {
        // Asymptotic expansion for large x: converges in 3-8 terms, much cheaper
        // than the CF which needs 15-30+ Lentz iterations at high x.
        //
        // Q(a, x) ≈ e^{-x} x^{a-1} / Γ(a) · [1 + (a-1)/x + (a-1)(a-2)/x² + ...]
        //
        // Compute asymptotic prefix in HP: log_prefix_asym = log_prefix - ln(x)
        let log_asym_hp = log_prefix_hp - ln_x_hp;
        if log_asym_hp < -40 * SCALE_HP {
            return Ok(0); // Q ≈ 0
        }
        let asym_prefix_hp = exp_fixed_hp(log_asym_hp)?;
        let asym_prefix = downscale_hp_to_std_i(asym_prefix_hp);
        return gamma_q_asymptotic(a, x, asym_prefix);
    }

    let prefix_hp = exp_fixed_hp(log_prefix_hp)?;
    let prefix = downscale_hp_to_std_i(prefix_hp);

    if x < a + SCALE_I {
        gamma_q_series(a, x, prefix)
    } else {
        gamma_q_cf(a, x, prefix)
    }
}

/// Series expansion for P(a, x). Returns Q = 1 - P.
///
/// Uses HP (1e15) arithmetic internally to minimize accumulated rounding error.
fn gamma_q_series(a: i128, x: i128, prefix: i128) -> Result<i128, SolMathError> {
    let a_hp = a * HP_TO_STD;
    let x_hp = x * HP_TO_STD;

    let mut term = fp_div_hp_safe(SCALE_HP, a_hp)?;
    let mut sum = term;

    const MAX_ITER: i32 = 100;
    const EPS_HP: i128 = HP_TO_STD;

    for n in 1..=MAX_ITER {
        let a_plus_n_hp = a_hp + n as i128 * SCALE_HP;
        term = fp_mul_hp_i(term, x_hp)?;
        term = fp_div_hp_safe(term, a_plus_n_hp)?;
        sum += term;

        if term.abs() < EPS_HP {
            break;
        }
    }

    let sum_std = downscale_hp_to_std_i(sum);
    let p = fp_mul_i_round(prefix, sum_std)?;
    let q = SCALE_I - p;
    Ok(q.clamp(0, SCALE_I))
}

/// Continued fraction for Q(a, x) via modified Lentz's method.
///
/// Uses HP (1e15) arithmetic internally for precision.
/// Q(a, x) = prefix / CF, where CF is the continued fraction value.
fn gamma_q_cf(a: i128, x: i128, prefix: i128) -> Result<i128, SolMathError> {
    let a_hp = a * HP_TO_STD;
    let x_hp = x * HP_TO_STD;

    const TINY: i128 = 1;
    const MAX_ITER: i32 = 80;
    const EPS_HP: i128 = HP_TO_STD;

    let b0 = x_hp + SCALE_HP - a_hp;
    let mut f = if b0.abs() < TINY { TINY } else { b0 };
    let mut c = f;
    let mut d: i128 = 0;

    for n in 1..=MAX_ITER {
        let n_hp = n as i128 * SCALE_HP;
        let a_n = fp_mul_hp_i(n_hp, a_hp - n_hp)?;
        let b_n = x_hp + (2 * n as i128 + 1) * SCALE_HP - a_hp;

        d = b_n + fp_mul_hp_i(a_n, d)?;
        if d.abs() < TINY {
            d = TINY;
        }
        d = fp_div_hp_safe(SCALE_HP, d)?;

        c = b_n + fp_div_hp_safe(a_n, c)?;
        if c.abs() < TINY {
            c = TINY;
        }

        let delta = fp_mul_hp_i(c, d)?;
        f = fp_mul_hp_i(f, delta)?;

        if (delta - SCALE_HP).abs() <= EPS_HP {
            break;
        }
    }

    let f_std = downscale_hp_to_std_i(f);
    let q = fp_div_i_round(prefix, f_std)?;
    Ok(q.clamp(0, SCALE_I))
}

/// Asymptotic expansion for Q(a, x) when x >> a.
///
/// Q(a, x) ≈ asym_prefix · S  where  asym_prefix = e^{-x} x^{a-1} / Γ(a)
/// S = 1 + (a-1)/x + (a-1)(a-2)/x² + ...
///
/// Converges in 3-8 terms for x ≥ 2a. Much cheaper than the CF (~30 Lentz steps).
/// The series is asymptotic (divergent), so we stop when terms start growing.
fn gamma_q_asymptotic(a: i128, x: i128, asym_prefix: i128) -> Result<i128, SolMathError> {
    // Compute sum S = 1 + (a-1)/x + (a-1)(a-2)/x² + ...
    // term_k = (a-1)(a-2)...(a-k) / x^k
    // Recurrence: term_k = term_{k-1} * (a - k) / x
    let mut sum = SCALE_I;
    let mut term = SCALE_I;
    let mut prev_abs = SCALE_I + 1; // sentinel > SCALE to allow first iteration

    for k in 1..=20i128 {
        let a_minus_k = a - k * SCALE_I;
        // term = term * (a - k) / x
        term = fp_mul_i_round(term, a_minus_k)?;
        term = fp_div_i_round(term, x)?;

        let term_abs = term.abs();
        // Stop if terms start growing (asymptotic divergence)
        if term_abs >= prev_abs {
            break;
        }
        sum += term;
        prev_abs = term_abs;

        // Also stop if term is negligible
        if term_abs == 0 {
            break;
        }
    }

    let q = fp_mul_i_round(asym_prefix, sum)?;
    Ok(q.clamp(0, SCALE_I))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ln_gamma tests ──

    #[test]
    fn ln_gamma_known_values() {
        let cases: &[(i128, i128, &str)] = &[
            (
                500_000_000_000,
                572_364_942_925,
                "ln_gamma(0.5) = ln(sqrt(pi))",
            ),
            (SCALE_I, 0, "ln_gamma(1.0) = 0"),
            (
                1_500_000_000_000,
                -120_782_237_635,
                "ln_gamma(1.5) = ln(sqrt(pi)/2)",
            ),
            (2 * SCALE_I, 0, "ln_gamma(2.0) = 0"),
            (2_500_000_000_000, 284_682_870_473, "ln_gamma(2.5)"),
            (3 * SCALE_I, 693_147_180_560, "ln_gamma(3.0) = ln(2)"),
            (5 * SCALE_I, 3_178_053_830_348, "ln_gamma(5.0) = ln(24)"),
            (
                10 * SCALE_I,
                12_801_827_480_081,
                "ln_gamma(10.0) = ln(362880)",
            ),
        ];

        for &(a, expected, label) in cases {
            let result = ln_gamma(a).unwrap();
            let err = (result - expected).abs();
            assert!(
                err <= 4,
                "{}: result={}, expected={}, err={} ULP",
                label,
                result,
                expected,
                err
            );
        }
    }

    #[test]
    fn ln_gamma_domain_errors() {
        assert_eq!(ln_gamma(0), Err(SolMathError::DomainError));
        assert_eq!(ln_gamma(499_999_999_999), Err(SolMathError::DomainError));
        assert_eq!(ln_gamma(10 * SCALE_I + 1), Err(SolMathError::DomainError));
        assert_eq!(ln_gamma(-SCALE_I), Err(SolMathError::DomainError));
    }

    #[test]
    fn ln_gamma_sweep() {
        // Sweep a = 0.5 to 10.0 in steps of 0.1, check monotonicity after minimum
        // ln_gamma decreases from a=0.5 to minimum near a=1.46, then increases.
        let mut prev = ln_gamma(500_000_000_000).unwrap();
        let mut found_min = false;
        for i in 6..=100 {
            let a = i as i128 * SCALE_I / 10;
            let val = ln_gamma(a).unwrap();
            if !found_min {
                if val > prev + 1 {
                    found_min = true;
                }
            }
            if found_min {
                assert!(
                    val >= prev - 1,
                    "non-monotone at a={}: {} < {}",
                    i,
                    val,
                    prev
                );
            }
            prev = val;
        }
        assert!(found_min, "minimum not found");
    }

    // ── regularized_gamma_q tests ──

    #[test]
    fn gamma_q_at_zero() {
        // Q(a, 0) = 1 for all a
        for a_10 in [5, 10, 15, 20, 50, 100] {
            let a = a_10 as i128 * SCALE_I / 10;
            assert_eq!(
                regularized_gamma_q(a, 0).unwrap(),
                SCALE_I,
                "Q({}/10, 0) should be 1",
                a_10
            );
        }
    }

    #[test]
    fn gamma_q_known_values() {
        let cases: &[(i128, i128, i128, &str)] = &[
            // (a_fp, x_fp, expected_Q_fp, label)
            // Q(1, x) = e^(-x)
            (SCALE_I, SCALE_I, 367_879_441_171, "Q(1,1) = e^-1"),
            (SCALE_I, 2 * SCALE_I, 135_335_283_237, "Q(1,2) = e^-2"),
            (SCALE_I, 5 * SCALE_I, 6_737_946_999, "Q(1,5) = e^-5"),
            // Q(0.5, x)
            (500_000_000_000, SCALE_I, 157_299_207_050, "Q(0.5,1)"),
            (500_000_000_000, 5 * SCALE_I, 1_565_402_258, "Q(0.5,5)"),
            // Q(2, x)
            (2 * SCALE_I, SCALE_I, 735_758_882_343, "Q(2,1)"),
            (2 * SCALE_I, 5 * SCALE_I, 40_427_681_995, "Q(2,5)"),
            // Q(5, x)
            (5 * SCALE_I, 5 * SCALE_I, 440_493_285_065, "Q(5,5)"),
            (5 * SCALE_I, 10 * SCALE_I, 29_252_688_077, "Q(5,10)"),
            // Q(10, x)
            (10 * SCALE_I, 10 * SCALE_I, 457_929_714_472, "Q(10,10)"),
            (10 * SCALE_I, 5 * SCALE_I, 968_171_942_694, "Q(10,5)"),
        ];

        for &(a, x, expected, label) in cases {
            let result = regularized_gamma_q(a, x).unwrap();
            let err = (result - expected).abs();
            assert!(
                err <= 4,
                "{}: result={}, expected={}, err={} ULP",
                label,
                result,
                expected,
                err
            );
        }
    }

    #[test]
    fn gamma_q_monotone_in_x() {
        // Q(a, x) should be monotonically decreasing in x
        let a = 3 * SCALE_I;
        let mut prev = SCALE_I;
        for x_10 in 1..=100 {
            let x = x_10 as i128 * SCALE_I / 10;
            let q = regularized_gamma_q(a, x).unwrap();
            assert!(q <= prev + 1, "Q(3, {}/10) = {} > prev = {}", x_10, q, prev);
            prev = q;
        }
    }

    #[test]
    fn gamma_q_domain_errors() {
        assert_eq!(
            regularized_gamma_q(0, SCALE_I),
            Err(SolMathError::DomainError)
        );
        assert_eq!(
            regularized_gamma_q(SCALE_I, -1),
            Err(SolMathError::DomainError)
        );
        assert_eq!(
            regularized_gamma_q(SCALE_I, 51 * SCALE_I),
            Err(SolMathError::DomainError)
        );
    }

    #[test]
    fn gamma_q_deep_tail() {
        // Q(0.5, 50) should be essentially 0
        let q = regularized_gamma_q(500_000_000_000, 50 * SCALE_I).unwrap();
        assert!(q <= 1, "Q(0.5, 50) = {} should be ~0", q);

        // Q(10, 0.1) should be essentially 1
        let q = regularized_gamma_q(10 * SCALE_I, 100_000_000_000).unwrap();
        assert!((q - SCALE_I).abs() <= 1, "Q(10, 0.1) = {} should be ~1", q);
    }

    #[test]
    fn gamma_q_grid() {
        // Broad grid test: a × x from mpmath at 50 digits
        // (a_fp, x_fp, expected_Q_fp)
        let grid: &[(i128, i128, i128)] = &[
            // a=0.5
            (500_000_000_000, 10_000_000_000, 887_537_083_982),
            (500_000_000_000, 100_000_000_000, 654_720_846_019),
            (500_000_000_000, 500_000_000_000, 317_310_507_863),
            (500_000_000_000, 2_000_000_000_000, 45_500_263_896),
            (500_000_000_000, 10_000_000_000_000, 7_744_216),
            // a=1.0
            (SCALE_I, 500_000_000_000, 606_530_659_713),
            (SCALE_I, 10_000_000_000_000, 45_399_930),
            // a=1.5
            (1_500_000_000_000, 500_000_000_000, 801_251_956_901),
            (1_500_000_000_000, 2_000_000_000_000, 261_464_129_949),
            (1_500_000_000_000, 5_000_000_000_000, 18_566_135_463),
            // a=2.0
            (2 * SCALE_I, 500_000_000_000, 909_795_989_569),
            (2 * SCALE_I, 2_000_000_000_000, 406_005_849_710),
            // a=3.0
            (3 * SCALE_I, SCALE_I, 919_698_602_929),
            (3 * SCALE_I, 5_000_000_000_000, 124_652_019_483),
            (3 * SCALE_I, 10_000_000_000_000, 2_769_395_716),
            // a=5.0
            (5 * SCALE_I, 2_000_000_000_000, 947_346_982_656),
            (5 * SCALE_I, 20_000_000_000_000, 16_944_744),
            // a=10.0
            (10 * SCALE_I, 2_000_000_000_000, 999_953_501_925),
            (10 * SCALE_I, 20_000_000_000_000, 4_995_412_308),
        ];

        let mut max_err: i128 = 0;
        for &(a, x, expected) in grid {
            let result = regularized_gamma_q(a, x).unwrap();
            let err = (result - expected).abs();
            if err > max_err {
                max_err = err;
            }
            assert!(
                err <= 4,
                "Q({}, {}): result={}, expected={}, err={} ULP",
                a,
                x,
                result,
                expected,
                err
            );
        }
    }

    #[test]
    fn gamma_q_crossover() {
        // Test near the x = a+1 crossover point (where we switch from series to CF)
        let a = 3 * SCALE_I;
        let x_below = a + SCALE_I - 1; // just below crossover
        let x_at = a + SCALE_I; // at crossover
        let q_below = regularized_gamma_q(a, x_below).unwrap();
        let q_at = regularized_gamma_q(a, x_at).unwrap();
        // Should be very close and Q decreasing
        assert!(
            q_at <= q_below + 1,
            "crossover discontinuity: {} > {}",
            q_at,
            q_below
        );
    }
}
