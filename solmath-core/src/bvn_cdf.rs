use crate::arithmetic::{fp_div_i, fp_mul_i};
use crate::constants::PI_OVER_2_SCALE;
use crate::error::SolMathError;
use crate::normal::norm_cdf_poly;
use crate::transcendental::exp_fixed_i;
use crate::trig::sincos_fixed;
use crate::SCALE_I;

// ── GL6 Gauss-Legendre nodes/weights (6-point, on [-1,1]) ──
// Drezner-Wesolowsky: < 10⁻⁴ accuracy for |ρ| < 0.95.
const GL6_NODES: [i128; 6] = [
    -932_469_514_203, // -0.932469514203152
    -661_209_386_466, // -0.661209386466265
    -238_619_186_083, // -0.238619186083197
    238_619_186_083,
    661_209_386_466,
    932_469_514_203,
];
const GL6_WEIGHTS: [i128; 6] = [
    171_324_492_379, // 0.171324492379170
    360_761_573_048, // 0.360761573048139
    467_913_934_573, // 0.467913934572691
    467_913_934_573,
    360_761_573_048,
    171_324_492_379,
];

// ── GL20 kept for offline table generation ──
const GL20_NODES: [i128; 20] = [
    -993_128_599_185,
    -963_971_927_278,
    -912_234_428_251,
    -839_116_971_822,
    -746_331_906_460,
    -636_053_680_727,
    -510_867_001_951,
    -373_706_088_715,
    -227_785_851_142,
    -76_526_521_133,
    76_526_521_133,
    227_785_851_142,
    373_706_088_715,
    510_867_001_951,
    636_053_680_727,
    746_331_906_460,
    839_116_971_822,
    912_234_428_251,
    963_971_927_278,
    993_128_599_185,
];
const GL20_WEIGHTS: [i128; 20] = [
    17_614_007_139,
    40_601_429_800,
    62_672_048_334,
    83_276_741_577,
    101_930_119_817,
    118_194_531_962,
    131_688_638_449,
    142_096_109_318,
    149_172_986_473,
    152_753_387_131,
    152_753_387_131,
    149_172_986_473,
    142_096_109_318,
    131_688_638_449,
    118_194_531_962,
    101_930_119_817,
    83_276_741_577,
    62_672_048_334,
    40_601_429_800,
    17_614_007_139,
];

const INV_TWO_PI: i128 = 159_154_943_092;
const RHO_NEAR_ONE: i128 = SCALE_I - 1_000_000; // 1 - 1e-6

#[inline]
fn clamp_prob(value: i128) -> i128 {
    value.clamp(0, SCALE_I)
}

fn asin_fixed(x: i128) -> Result<i128, SolMathError> {
    if x.abs() > SCALE_I {
        return Err(SolMathError::DomainError);
    }
    if x == SCALE_I {
        return Ok(PI_OVER_2_SCALE);
    }
    if x == -SCALE_I {
        return Ok(-PI_OVER_2_SCALE);
    }

    let x2 = fp_mul_i(x, x)?;
    let x3 = fp_mul_i(x2, x)?;
    let x5 = fp_mul_i(x3, x2)?;
    let x7 = fp_mul_i(x5, x2)?;

    let mut theta = x
        .checked_add(x3 / 6)
        .ok_or(SolMathError::Overflow)?
        .checked_add((3 * x5) / 40)
        .ok_or(SolMathError::Overflow)?
        .checked_add((5 * x7) / 112)
        .ok_or(SolMathError::Overflow)?;

    for _ in 0..5 {
        let (sin_theta, cos_theta) = sincos_fixed(theta)?;
        if cos_theta == 0 {
            return Ok(theta.clamp(-PI_OVER_2_SCALE, PI_OVER_2_SCALE));
        }
        let error = sin_theta.checked_sub(x).ok_or(SolMathError::Overflow)?;
        if error.abs() <= 4 {
            return Ok(theta.clamp(-PI_OVER_2_SCALE, PI_OVER_2_SCALE));
        }
        let step = fp_div_i(error, cos_theta)?;
        theta = theta
            .checked_sub(step)
            .ok_or(SolMathError::Overflow)?
            .clamp(-PI_OVER_2_SCALE, PI_OVER_2_SCALE);
    }

    Ok(theta.clamp(-PI_OVER_2_SCALE, PI_OVER_2_SCALE))
}

/// Core quadrature for x ≤ 0, y ≤ 0.
#[inline]
fn direct_cdf_negative_gl(
    x: i128,
    y: i128,
    rho: i128,
    nodes: &[i128],
    weights: &[i128],
) -> Result<i128, SolMathError> {
    let phi_x = norm_cdf_poly(x)?;
    let phi_y = norm_cdf_poly(y)?;
    let base = fp_mul_i(phi_x, phi_y)?;
    if rho == 0 {
        return Ok(base);
    }

    let alpha = asin_fixed(rho)?;
    let half = alpha / 2;
    let mid = alpha / 2;
    let x_sq = fp_mul_i(x, x)?;
    let y_sq = fp_mul_i(y, y)?;
    let xy = fp_mul_i(x, y)?;
    let mut weighted_sum = 0i128;

    for idx in 0..nodes.len() {
        let theta = mid
            .checked_add(fp_mul_i(half, nodes[idx])?)
            .ok_or(SolMathError::Overflow)?;
        let (sin_theta, cos_theta) = sincos_fixed(theta)?;
        let cos_sq = fp_mul_i(cos_theta, cos_theta)?;
        if cos_sq <= 0 {
            return Err(SolMathError::DomainError);
        }
        let cross = 2_i128
            .checked_mul(fp_mul_i(xy, sin_theta)?)
            .ok_or(SolMathError::Overflow)?;
        let numerator = x_sq
            .checked_sub(cross)
            .ok_or(SolMathError::Overflow)?
            .checked_add(y_sq)
            .ok_or(SolMathError::Overflow)?;
        let denominator = 2_i128.checked_mul(cos_sq).ok_or(SolMathError::Overflow)?;
        let exponent = -fp_div_i(numerator, denominator)?;
        let exp_term = exp_fixed_i(exponent)?;
        let weighted = fp_mul_i(weights[idx], exp_term)?;
        weighted_sum = weighted_sum
            .checked_add(weighted)
            .ok_or(SolMathError::Overflow)?;
    }

    let integral = fp_mul_i(half, weighted_sum)?;
    let correction = fp_mul_i(INV_TWO_PI, integral)?;
    Ok(clamp_prob(
        base.checked_add(correction).ok_or(SolMathError::Overflow)?,
    ))
}

/// Quadrant-folding dispatch shared by GL6 and GL20 paths.
fn bvn_cdf_with_gl(
    x: i128,
    y: i128,
    rho: i128,
    nodes: &[i128],
    weights: &[i128],
) -> Result<i128, SolMathError> {
    if rho.abs() > SCALE_I {
        return Err(SolMathError::DomainError);
    }
    if rho >= RHO_NEAR_ONE {
        return norm_cdf_poly(x.min(y));
    }
    if rho <= -RHO_NEAR_ONE {
        let value = norm_cdf_poly(x)?
            .checked_add(norm_cdf_poly(y)?)
            .ok_or(SolMathError::Overflow)?
            .checked_sub(SCALE_I)
            .ok_or(SolMathError::Overflow)?;
        return Ok(clamp_prob(value));
    }

    if x > 0 && y > 0 {
        let fx = norm_cdf_poly(x)?;
        let fy = norm_cdf_poly(y)?;
        let tail = bvn_cdf_with_gl(-x, -y, rho, nodes, weights)?;
        let value = fx
            .checked_add(fy)
            .ok_or(SolMathError::Overflow)?
            .checked_sub(SCALE_I)
            .ok_or(SolMathError::Overflow)?
            .checked_add(tail)
            .ok_or(SolMathError::Overflow)?;
        return Ok(clamp_prob(value));
    }
    if x > 0 {
        let fy = norm_cdf_poly(y)?;
        let tail = bvn_cdf_with_gl(-x, y, -rho, nodes, weights)?;
        return Ok(clamp_prob(
            fy.checked_sub(tail).ok_or(SolMathError::Overflow)?,
        ));
    }
    if y > 0 {
        let fx = norm_cdf_poly(x)?;
        let tail = bvn_cdf_with_gl(x, -y, -rho, nodes, weights)?;
        return Ok(clamp_prob(
            fx.checked_sub(tail).ok_or(SolMathError::Overflow)?,
        ));
    }

    direct_cdf_negative_gl(x, y, rho, nodes, weights)
}

// ═══════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════

/// General bivariate normal CDF. Any `ρ`. ~100K CU. Accuracy < 10⁻⁴.
///
/// Computes `P(X ≤ a, Y ≤ b)` where `(X, Y) ~ N(0, 0, 1, 1, ρ)`.
///
/// All inputs/outputs are signed fixed-point `i64` at `SCALE` (1e12).
/// `rho` must lie in `[-SCALE, SCALE]`.
///
/// Uses 6-point Gauss-Legendre quadrature (Drezner-Wesolowsky).
/// Accuracy is < 10⁻⁴ for `|ρ| < 0.95` and smoothly degrades beyond.
/// Near `ρ = ±1` the routine switches to the analytic limit.
pub fn bvn_cdf(a: i64, b: i64, rho: i64) -> Result<i64, SolMathError> {
    let result = bvn_cdf_with_gl(a as i128, b as i128, rho as i128, &GL6_NODES, &GL6_WEIGHTS)?;
    Ok(result as i64)
}

/// High-precision bivariate normal CDF. Any `ρ`. ~331K CU. Accuracy < 10⁻⁶.
///
/// 20-point Gauss-Legendre. Use offline for table generation and validation.
/// Not recommended on-chain — use [`bvn_cdf`] (GL6) or
/// [`bvn_cdf_fast`](super::bvn_table::bvn_cdf_fast) (table lookup) instead.
pub(crate) fn bvn_cdf_gl20(x: i128, y: i128, rho: i128) -> Result<i128, SolMathError> {
    bvn_cdf_with_gl(x, y, rho, &GL20_NODES, &GL20_WEIGHTS)
}

/// High-precision bivariate normal CDF with `i64` interface. ~331K CU.
///
/// 20-point Gauss-Legendre. Accuracy < 10⁻⁶. Use for offline validation
/// and as a reference for the GL6 [`bvn_cdf`].
///
/// # Domain
///
/// `a`, `b` ∈ `[-4·SCALE, 4·SCALE]`, `rho` ∈ `(-SCALE, SCALE)`.
///
/// # Errors
///
/// - `DomainError` if `|rho| > SCALE`.
/// - `Overflow` from internal fixed-point operations (extreme inputs).
pub fn bvn_cdf_hp(a: i64, b: i64, rho: i64) -> Result<i64, SolMathError> {
    let result = bvn_cdf_gl20(a as i128, b as i128, rho as i128)?;
    Ok(result as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SCALE;

    fn phi(x: f64) -> f64 {
        0.398_942_280_401_432_7 * (-0.5 * x * x).exp()
    }

    fn std_norm_cdf_ref(x: f64) -> f64 {
        if x < -10.0 {
            return 0.0;
        }
        if x > 10.0 {
            return 1.0;
        }
        let n = 4_096usize;
        let left = -10.0f64;
        let right = x;
        let h = (right - left) / n as f64;
        let mut sum = phi(left) + phi(right);
        for i in 1..n {
            let xi = left + h * i as f64;
            sum += if i % 2 == 0 {
                2.0 * phi(xi)
            } else {
                4.0 * phi(xi)
            };
        }
        (sum * h / 3.0).clamp(0.0, 1.0)
    }

    fn simpson_integrate<F: Fn(f64) -> f64>(f: F, a: f64, b: f64) -> f64 {
        let n = 4_096usize;
        let h = (b - a) / n as f64;
        let mut sum = f(a) + f(b);
        for i in 1..n {
            let xi = a + h * i as f64;
            sum += if i % 2 == 0 { 2.0 * f(xi) } else { 4.0 * f(xi) };
        }
        sum * h / 3.0
    }

    fn bvn_cdf_ref(x: f64, y: f64, rho: f64) -> f64 {
        if rho >= 0.999_999 {
            return std_norm_cdf_ref(x.min(y));
        }
        if rho <= -0.999_999 {
            return (std_norm_cdf_ref(x) + std_norm_cdf_ref(y) - 1.0).clamp(0.0, 1.0);
        }
        if x > 0.0 && y > 0.0 {
            return (std_norm_cdf_ref(x) + std_norm_cdf_ref(y) - 1.0 + bvn_cdf_ref(-x, -y, rho))
                .clamp(0.0, 1.0);
        }
        if x > 0.0 {
            return (std_norm_cdf_ref(y) - bvn_cdf_ref(-x, y, -rho)).clamp(0.0, 1.0);
        }
        if y > 0.0 {
            return (std_norm_cdf_ref(x) - bvn_cdf_ref(x, -y, -rho)).clamp(0.0, 1.0);
        }
        let alpha = rho.asin();
        let base = std_norm_cdf_ref(x) * std_norm_cdf_ref(y);
        let integral = simpson_integrate(
            |theta| {
                let sin_theta = theta.sin();
                let cos_theta = theta.cos();
                let exponent =
                    -(x * x - 2.0 * x * y * sin_theta + y * y) / (2.0 * cos_theta * cos_theta);
                exponent.exp()
            },
            0.0,
            alpha,
        );
        (base + integral / (2.0 * core::f64::consts::PI)).clamp(0.0, 1.0)
    }

    /// GL20 (hp) matches f64 reference to < 1e-6.
    #[test]
    fn bvn_cdf_gl20_matches_reference_grid() {
        let xs = [-2.5, -1.25, -0.5, 0.0, 0.4, 1.1, 2.2];
        let ys = [-2.0, -0.75, 0.0, 0.5, 1.4];
        let rhos = [-0.95, -0.6, -0.2, 0.0, 0.3, 0.7, 0.95];
        let mut max_err = 0.0f64;
        for &x in &xs {
            for &y in &ys {
                for &rho in &rhos {
                    let got = bvn_cdf_hp(
                        (x * SCALE as f64).round() as i64,
                        (y * SCALE as f64).round() as i64,
                        (rho * SCALE as f64).round() as i64,
                    )
                    .expect("bvn_cdf_hp") as f64
                        / SCALE as f64;
                    let want = bvn_cdf_ref(x, y, rho);
                    let err = (got - want).abs();
                    if err > max_err {
                        max_err = err;
                    }
                }
            }
        }
        assert!(max_err < 1.0e-6, "max_err={max_err}");
    }

    /// GL20 (hp) matches f64 reference on 1024 random points.
    #[test]
    fn bvn_cdf_gl20_matches_reference_random_pack() {
        let mut state = 0x1234_5678_9abc_def0u64;
        let mut max_err = 0.0f64;
        for _ in 0..1_024 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let x = (((state >> 11) % 8_001) as i64 - 4_000) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let y = (((state >> 9) % 8_001) as i64 - 4_000) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let rho = (((state >> 13) % 1_901) as i64 - 950) as f64 / 1_000.0;
            let got = bvn_cdf_hp(
                (x * SCALE as f64).round() as i64,
                (y * SCALE as f64).round() as i64,
                (rho * SCALE as f64).round() as i64,
            )
            .expect("bvn_cdf_hp") as f64
                / SCALE as f64;
            let want = bvn_cdf_ref(x, y, rho);
            let err = (got - want).abs();
            if err > max_err {
                max_err = err;
            }
        }
        assert!(max_err < 1.0e-6, "max_err={max_err}");
    }

    /// GL6 matches f64 reference to < 1e-4 for |ρ| < 0.95.
    #[test]
    fn bvn_cdf_gl6_accuracy() {
        let xs = [-2.5, -1.25, -0.5, 0.0, 0.4, 1.1, 2.2];
        let ys = [-2.0, -0.75, 0.0, 0.5, 1.4];
        let rhos = [-0.9, -0.6, -0.2, 0.0, 0.3, 0.7, 0.9];
        let mut max_err = 0.0f64;
        for &x in &xs {
            for &y in &ys {
                for &rho in &rhos {
                    let got = bvn_cdf(
                        (x * SCALE as f64).round() as i64,
                        (y * SCALE as f64).round() as i64,
                        (rho * SCALE as f64).round() as i64,
                    )
                    .expect("bvn_cdf") as f64
                        / SCALE as f64;
                    let want = bvn_cdf_ref(x, y, rho);
                    let err = (got - want).abs();
                    if err > max_err {
                        max_err = err;
                    }
                }
            }
        }
        std::eprintln!("GL6 max error (|ρ|≤0.9): {max_err:.2e}");
        assert!(max_err < 1.0e-4, "max_err={max_err}");
    }

    /// GL6 on 1024 random points with |ρ| < 0.95.
    #[test]
    fn bvn_cdf_gl6_random_pack() {
        let mut state = 0x1234_5678_9abc_def0u64;
        let mut max_err = 0.0f64;
        for _ in 0..1_024 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let x = (((state >> 11) % 8_001) as i64 - 4_000) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let y = (((state >> 9) % 8_001) as i64 - 4_000) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            // |ρ| < 0.95
            let rho = (((state >> 13) % 1_901) as i64 - 950) as f64 / 1_000.0;
            let got = bvn_cdf(
                (x * SCALE as f64).round() as i64,
                (y * SCALE as f64).round() as i64,
                (rho * SCALE as f64).round() as i64,
            )
            .expect("bvn_cdf") as f64
                / SCALE as f64;
            let want = bvn_cdf_ref(x, y, rho);
            let err = (got - want).abs();
            if err > max_err {
                max_err = err;
            }
        }
        std::eprintln!("GL6 max error (random, |ρ|<0.95): {max_err:.2e}");
        assert!(max_err < 1.0e-4, "max_err={max_err}");
    }

    /// G: |ρ| > SCALE triggers DomainError.
    #[test]
    fn bvn_cdf_rejects_rho_above_scale() {
        let s = SCALE as i64;
        assert_eq!(bvn_cdf(0, 0, s + 1), Err(SolMathError::DomainError));
        assert_eq!(bvn_cdf(0, 0, -(s + 1)), Err(SolMathError::DomainError));
    }

    /// H: ρ = exactly ±SCALE hits the analytic ρ→±1 branch, not the
    /// quadrature. Verify no panic and returns a valid probability.
    #[test]
    fn bvn_cdf_at_rho_exactly_one() {
        let s = SCALE as i64;
        // ρ = +1: Φ₂(a,b;1) = Φ(min(a,b))
        let v = bvn_cdf(s, 0, s).unwrap();
        assert!(v > 0 && v <= s);
        // ρ = -1: Φ₂(a,b;-1) = max(Φ(a)+Φ(b)-1, 0)
        let v = bvn_cdf(0, 0, -s).unwrap();
        assert!(v >= 0 && v <= s);
    }
}
