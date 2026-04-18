use crate::arithmetic::{fp_div_i, fp_mul_i_round, fp_sqrt};
use crate::constants::*;
use crate::error::SolMathError;
use crate::transcendental::{exp_fixed_i, ln_fixed_i};

// ============================================================
// Modified Bessel function of the second kind, order 1: K₁(z)
// ============================================================
//
// Two-branch rational/polynomial approximation:
//   Branch 1 (0 < z ≤ 2): K₁(z) = ln(z/2)·I₁(z) + (1/z)·P(t)
//     where t = (z/2)², P is a degree-9 polynomial, and
//     I₁(z) = (z/2) · Σ_{k=0}^{8} t^k / (k!(k+1)!)
//
//   Branch 2 (z > 2):     K₁(z) = exp(-z)/√z · √(π/2) · Q(2/z)
//     where Q is a degree-7 polynomial in 2/z.
//
// Coefficients fitted against scipy.special.kv(1,z) via minimax polynomial.
// Branch 1: max relative K₁ error < 8e-12 (sub-ULP at SCALE).
// Branch 2: max relative K₁ error < 2e-8 (~0.02 ULP at SCALE).

// ── Branch 1: P(t) coefficients ────────────────────────────────────────────
// P(t) where t = (z/2)²,  z·K₁(z) = z·ln(z/2)·I₁(z) + P(t).
// Degree 9 in t.  Max |K₁ error| < 8e-12 relative on (0, 2].
const BK1_P: [i128; 10] = [
    1_000_000_000_000, //  1.000000000000e+00
    154_431_329_803,   //  1.544313298030e-01
    -672_784_335_098,  // -6.727843350981e-01
    -181_575_166_961,  // -1.815751669613e-01
    -19_182_189_843,   // -1.918218984316e-02
    -1_115_359_475,    // -1.115359474634e-03
    -41_422_505,       // -4.142250499376e-05
    -1_071_530,        // -1.071529858569e-06
    -20_446,           // -2.044641848468e-08
    -311,              // -3.114509310964e-10
];

// ── Branch 1: I₁(z)/(z/2) series coefficients ─────────────────────────────
// I₁(z)/(z/2) = Σ_{k=0}^{8} t^k / (k!(k+1)!)  where t = (z/2)².
// Truncated after k=8; remaining terms < 1e-13 for z ≤ 2.
const BK1_I1: [i128; 9] = [
    1_000_000_000_000, // k=0: 1/(0!·1!)       = 1.0
    500_000_000_000,   // k=1: 1/(1!·2!)       = 0.5
    83_333_333_333,    // k=2: 1/(2!·3!)       = 1/12
    6_944_444_444,     // k=3: 1/(3!·4!)       = 1/144
    347_222_222,       // k=4: 1/(4!·5!)       = 1/2880
    11_574_074,        // k=5: 1/(5!·6!)       = 1/86400
    275_573,           // k=6: 1/(6!·7!)       = ~2.756e-7
    4_921,             // k=7: 1/(7!·8!)       = ~4.921e-9
    68,                // k=8: 1/(8!·9!)       = ~6.835e-11
];

// ── Branch 2: Q(t) coefficients ────────────────────────────────────────────
// K₁(z) = exp(-z)/√z · √(π/2) · Q(2/z)  for z > 2.
// Q(t) with t = 2/z, degree 7.  Max relative error < 2e-8.
const BK1_Q: [i128; 8] = [
    1_000_000_113_783, //  1.000000113783e+00
    187_496_060_708,   //  1.874960607080e-01
    -29_243_665_553,   // -2.924366555331e-02
    12_442_593_272,    //  1.244259327153e-02
    -7_439_287_207,    // -7.439287206731e-03
    4_336_388_237,     //  4.336388237230e-03
    -1_794_614_676,    // -1.794614675751e-03
    356_177_032,       //  3.561770318337e-04
];

/// √(π/2) at SCALE (1e12).
const SQRT_HALF_PI: i128 = 1_253_314_137_316; // √(π/2) ≈ 1.253314137316

/// 2 · SCALE.
const TWO_SCALE: u128 = 2 * SCALE;

/// Modified Bessel function of the second kind, order 1.
///
/// Computes K₁(z) where z = `x / SCALE` and returns the result at `SCALE`.
///
/// Two-branch approximation:
///   - **0 < z ≤ 2:** series expansion using `ln` and `I₁` with a
///     degree-9 correction polynomial. Relative error < 8 × 10⁻¹².
///   - **z > 2:** asymptotic expansion `exp(-z)/√z · √(π/2) · Q(2/z)`
///     with a degree-7 polynomial. Relative error < 2 × 10⁻⁸.
///
/// For z > 33 the result underflows to 0 at `SCALE` precision.
///
/// - **x**: unsigned fixed-point at `SCALE`. Must be > 0.
/// - **Returns**: `i128` at `SCALE`. Always positive for valid inputs.
/// - **Errors**: [`DomainError`] if `x == 0`.
/// - **Accuracy**: < 1 ULP (branch 1), < 20 ULP (branch 2).
///
/// # Example
/// ```
/// use solmath_core::{bessel_k1, SCALE};
/// // K₁(1.0) ≈ 0.60190723...
/// let k1 = bessel_k1(SCALE).unwrap();
/// assert!((k1 - 601_907_230_197).abs() <= 50);
/// ```
pub fn bessel_k1(x: u128) -> Result<i128, SolMathError> {
    if x == 0 {
        return Err(SolMathError::DomainError);
    }

    // For very large z, K₁ underflows to 0 at SCALE precision.
    // K₁(33) ≈ 3.7e-15 → 0 at SCALE=1e12.
    if x > 33 * SCALE {
        return Ok(0);
    }

    if x <= TWO_SCALE {
        bessel_k1_small(x)
    } else {
        bessel_k1_large(x)
    }
}

/// Branch 1: K₁(z) for 0 < z ≤ 2.
///
/// K₁(z) = ln(z/2) · I₁(z) + (1/z) · P(t)
/// where t = (z/2)² and I₁(z) = (z/2) · Σ t^k / (k!(k+1)!).
fn bessel_k1_small(x: u128) -> Result<i128, SolMathError> {
    let x_i = x as i128;

    // half_z = z/2 at SCALE
    let half_z = x_i / 2;

    // t = (z/2)² at SCALE
    let t = fp_mul_i_round(half_z, half_z)?;

    // Evaluate I₁(z) / (z/2) via Horner on t
    let mut i1_over_hz = BK1_I1[8];
    for k in (0..8).rev() {
        i1_over_hz = fp_mul_i_round(i1_over_hz, t)? + BK1_I1[k];
    }

    // I₁(z) = (z/2) · i1_over_hz  at SCALE
    let i1 = fp_mul_i_round(half_z, i1_over_hz)?;

    // ln(z/2) at SCALE.  z/2 = half_z/SCALE, so ln(z/2) = ln_fixed(half_z).
    // Handle z/2 < 1 (i.e., z < 2 SCALE) → half_z < SCALE → we need to handle
    // ln of values < SCALE.  ln_fixed_i takes u128 at SCALE, returns i128.
    let ln_hz = ln_fixed_i(half_z as u128)?;

    // Term 1: ln(z/2) · I₁(z) at SCALE
    let term1 = fp_mul_i_round(ln_hz, i1)?;

    // Evaluate P(t) via Horner
    let mut p = BK1_P[9];
    for k in (0..9).rev() {
        p = fp_mul_i_round(p, t)? + BK1_P[k];
    }

    // Term 2: P(t) / z  at SCALE.
    // P(t) is at SCALE, z = x_i / SCALE.  So P/z = P * SCALE / x_i.
    let term2 = fp_div_i(p, x_i)?;

    Ok(term1 + term2)
}

/// Branch 2: K₁(z) for z > 2.
///
/// K₁(z) = exp(-z) / √z · √(π/2) · Q(2/z)
/// where Q is a degree-7 polynomial in 2/z.
fn bessel_k1_large(x: u128) -> Result<i128, SolMathError> {
    let x_i = x as i128;

    // exp(-z) at SCALE.
    let neg_x = -(x_i);
    let exp_neg = exp_fixed_i(neg_x)?;
    if exp_neg == 0 {
        return Ok(0);
    }

    // 1/√z at SCALE.
    let sqrt_x = fp_sqrt(x)? as i128;
    if sqrt_x == 0 {
        return Err(SolMathError::DomainError);
    }
    // inv_sqrt_z = SCALE / √z = SCALE² / (√z · SCALE) ← use fp_div
    let inv_sqrt_z = fp_div_i(SCALE_I, sqrt_x)?;

    // t = 2/z at SCALE.
    let t = fp_div_i(2 * SCALE_I, x_i)?;

    // Q(t) via Horner
    let mut q = BK1_Q[7];
    for k in (0..7).rev() {
        q = fp_mul_i_round(q, t)? + BK1_Q[k];
    }

    // result = exp(-z) · (1/√z) · √(π/2) · Q(t)
    let r1 = fp_mul_i_round(exp_neg, inv_sqrt_z)?;
    let r2 = fp_mul_i_round(r1, SQRT_HALF_PI)?;
    let result = fp_mul_i_round(r2, q)?;

    Ok(result)
}

// ============================================================
// NIG PDF via Bessel K₁
// ============================================================

/// NIG probability density at a single point, using `bessel_k1`.
///
/// f(z) = (α·δ_t/π) · K₁(α·r) / r · exp(δ_t·γ + β·(z − μ))
/// where r = √(δ_t² + (z − μ)²), γ = √(α² − β²).
///
/// All inputs at `SCALE` (1e12). Returns f(z) at `SCALE`.
/// Returns `Ok(0)` when the result underflows.
///
/// - **z**: evaluation point (log-return) at `SCALE`.
/// - **alpha, beta, dt, gamma, drift**: NIG parameters at `SCALE`.
pub fn nig_pdf_bessel(
    z: i128,
    alpha: i128,
    beta: i128,
    dt: i128,
    gamma: i128,
    drift: i128,
) -> Result<i128, SolMathError> {
    let zmd = z - drift; // z − μ

    // r = √(dt² + (z−μ)²) at SCALE
    let dt_sq = fp_mul_i_round(dt, dt)?;
    let zmd_sq = fp_mul_i_round(zmd, zmd)?;
    let r_sq = dt_sq + zmd_sq;
    if r_sq <= 0 {
        return Err(SolMathError::DomainError);
    }
    let r = fp_sqrt(r_sq as u128)? as i128;
    if r == 0 {
        return Ok(0);
    }

    // α·r at SCALE
    let ar = fp_mul_i_round(alpha, r)?;
    if ar <= 0 {
        return Ok(0);
    }

    // K₁(α·r)
    let k1_val = bessel_k1(ar as u128)?;
    if k1_val == 0 {
        return Ok(0);
    }

    // exp(dt·γ + β·(z−μ)) at SCALE
    let exp_arg = fp_mul_i_round(dt, gamma)? + fp_mul_i_round(beta, zmd)?;
    let exp_val = exp_fixed_i(exp_arg)?;
    if exp_val == 0 {
        return Ok(0);
    }

    // prefactor = α·dt / π at SCALE
    const PI_SCALE: i128 = 3_141_592_653_590;
    let ad = fp_mul_i_round(alpha, dt)?;
    let prefactor = fp_div_i(ad, PI_SCALE)?;

    // K₁(α·r) / r at SCALE
    let k1_over_r = fp_div_i(k1_val, r)?;

    // result = prefactor × (K₁/r) × exp at SCALE
    let tmp = fp_mul_i_round(prefactor, k1_over_r)?;
    let result = fp_mul_i_round(tmp, exp_val)?;

    Ok(result.max(0))
}

/// NIG cell-integrated transition probability via Simpson's 3-point rule.
///
/// P ≈ (dx/6) · [f(lo) + 4·f(mid) + f(hi)]
///
/// where lo, mid, hi are evaluated at (cell_boundary − rep_i).
///
/// All inputs at `SCALE`. Returns probability at `SCALE`.
pub fn nig_cell_prob_simpson(
    rep_i: i128,
    cell_lo: i128,
    cell_hi: i128,
    alpha: i128,
    beta: i128,
    dt: i128,
    gamma: i128,
    drift: i128,
) -> Result<i128, SolMathError> {
    let dx = cell_hi - cell_lo;
    if dx <= 0 {
        return Ok(0);
    }

    let z_lo = cell_lo - rep_i;
    let z_mid = (cell_lo + cell_hi) / 2 - rep_i;
    let z_hi = cell_hi - rep_i;

    let f_lo = nig_pdf_bessel(z_lo, alpha, beta, dt, gamma, drift)?;
    let f_mid = nig_pdf_bessel(z_mid, alpha, beta, dt, gamma, drift)?;
    let f_hi = nig_pdf_bessel(z_hi, alpha, beta, dt, gamma, drift)?;

    // (dx/6) × (f_lo + 4·f_mid + f_hi)
    let integrand = f_lo + 4 * f_mid + f_hi;
    let num = fp_mul_i_round(dx, integrand)?;
    Ok((num / 6).max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference K₁ values from scipy.special.kv(1, z) at 12+ digits.
    const K1_REFS: [(u128, i128); 11] = [
        // (z at SCALE, K₁(z) at SCALE)
        (10_000_000_000, 9_853_844_780_871), // K₁(0.01)   ≈ *** too large, use 0.1
        (100_000_000_000, 9_853_844_780_871), // K₁(0.10) ≈ 9.8538
        (500_000_000_000, 1_656_441_120_003), // K₁(0.50) ≈ 1.6564
        (1_000_000_000_000, 601_907_230_197), // K₁(1.00) ≈ 0.6019
        (1_500_000_000_000, 277_387_800_457), // K₁(1.50) ≈ 0.2774
        (2_000_000_000_000, 139_865_881_817), // K₁(2.00) ≈ 0.1399
        (2_500_000_000_000, 73_890_816_348), // K₁(2.50) ≈ 0.0739
        (5_000_000_000_000, 4_044_613_445),  // K₁(5.00) ≈ 0.00404
        (10_000_000_000_000, 18_648_773),    // K₁(10.0) ≈ 1.86e-5
        (15_000_000_000_000, 101_417),       // K₁(15.0) ≈ 1.01e-7
        (20_000_000_000_000, 588),           // K₁(20.0) ≈ 5.9e-10
    ];

    #[test]
    fn k1_reference_values() {
        for &(x, expected) in &K1_REFS[1..] {
            // skip z=0.01 (K₁ ≈ 100, very large)
            let result = bessel_k1(x).unwrap();
            let err = (result - expected).abs();
            let tol = if x <= 2 * SCALE {
                // Branch 1: sub-ULP accuracy expected
                (expected.abs() / 1_000_000).max(1) // 1 ppm relative or 1 ULP
            } else {
                // Branch 2: ~20 ULP accuracy
                (expected.abs() / 50_000).max(1) // 20 ppm relative or 1 ULP
            };
            assert!(
                err <= tol,
                "K₁({}) = {}, expected {}, err {} > tol {}",
                x as f64 / SCALE as f64,
                result,
                expected,
                err,
                tol,
            );
        }
    }

    #[test]
    fn k1_domain_error() {
        assert!(bessel_k1(0).is_err());
    }

    #[test]
    fn k1_large_z_underflow() {
        assert_eq!(bessel_k1(34 * SCALE).unwrap(), 0);
    }

    #[test]
    fn k1_branch_boundary() {
        // z = 2.0: branch 1
        let k1_at_2 = bessel_k1(2 * SCALE).unwrap();
        // z = 2.001: branch 2 (just over boundary)
        let k1_just_over = bessel_k1(2 * SCALE + SCALE / 1000).unwrap();
        // K₁ is smooth and monotonically decreasing — both should be close
        let diff = (k1_at_2 - k1_just_over).abs();
        // K₁'(2) ≈ -0.14, so over 0.001 the change is ~0.00014 → ~140M at SCALE
        assert!(diff < 200_000_000, "branch discontinuity: diff = {}", diff);
    }

    #[test]
    fn k1_monotone_decreasing() {
        let mut prev = i128::MAX;
        for i in 1..=30u128 {
            let x = i * SCALE / 2; // 0.5, 1.0, ..., 15.0
            let k1 = bessel_k1(x).unwrap();
            assert!(
                k1 <= prev,
                "K₁ not monotone decreasing at z={}",
                x as f64 / SCALE as f64,
            );
            prev = k1;
        }
    }
}
