// N=3 CV premium engine.
//
// Two-transaction engine for N=3 impermanent loss insurance premium.
// Premium = analytical E[(Q-d)⁺ - (Q-c)⁺] + tensor 9×5×5 correction.
//
// Public API:
//   eigendecompose_2x2    — Gate 1
//   chi2_call              — Gate 2
//   spread_split           — Gate 3
//   n3_cv_setup            — Gate 5, tx1
//   n3_cv_correction       — Gate 5, tx2
//   n3_cv_premium          — Gate 5, combined
//   compute_pool_constants — Pool PDA setup (weight-only, once)
//   compute_n3_premium     — Single-tx engine (~971K CU)

use crate::arithmetic::{fp_div_i, fp_mul_i, fp_sqrt};
use crate::constants::*;
use crate::error::SolMathError;
use crate::gauss_hermite::{
    GH3_WEIGHTS, GH5_WEIGHTS, GH7_WEIGHTS, GL5_NODES, GL5_WEIGHTS, GL7_NODES, GL7_WEIGHTS,
    INV_SQRT_PI,
};
use crate::normal::{norm_cdf_and_pdf, norm_cdf_poly, norm_pdf};
use crate::transcendental::exp_fixed_i;
use crate::trig::cos_fixed;

// ─── Constants ────────────────────────────────────────────────────────────────

/// √2 at SCALE (kept for reference; SQRT2_GH5/GH9 fold this in).
#[allow(dead_code)]
const SQRT2: i128 = 1_414_213_562_373;

/// 1/π at SCALE.
const INV_PI: i128 = 318_309_886_184;

/// √2 × GH5_NODES (physicist convention), pre-scaled for inner quadrature.
const SQRT2_GH5: [i128; 5] = [
    -2_856_970_013_873,
    -1_355_626_179_974,
    0,
    1_355_626_179_974,
    2_856_970_013_873,
];

/// √2 × GH7_NODES (physicist convention), pre-scaled for outer quadrature.
const SQRT2_GH7: [i128; 7] = [
    -3_750_439_717_726,
    -2_366_759_410_735,
    -1_154_405_394_740,
    0,
    1_154_405_394_740,
    2_366_759_410_735,
    3_750_439_717_726,
];

/// sin²(θ_j) where θ_j = (π/4)(1 + GL7_NODES[j]), for the sin² substitution.
const GL7_SIN2_THETA: [i128; 7] = [
    1_596_794_399,
    40_646_408_672,
    202_405_801_746,
    500_000_000_000,
    797_594_198_254,
    959_353_591_328,
    998_403_205_601,
];

/// sin(2θ_j) for the sin² substitution jacobian.
const GL7_SIN_2THETA: [i128; 7] = [
    79_855_986_551,
    394_939_378_307,
    803_586_194_917,
    1_000_000_000_000,
    803_586_194_917,
    394_939_378_307,
    79_855_986_551,
];

/// GL7 weights × π/4 (mapped to [0, π/2]).
const GL7_WEIGHTS_PI4: [i128; 7] = [
    101_697_254_617,
    219_680_100_768,
    299_888_620_397,
    328_264_375_232,
    299_888_620_397,
    219_680_100_768,
    101_697_254_617,
];

// ─── GL5 sin² substitution constants ─────────────────────────────────────────

/// sin²(θ_j) for GL5 sin² substitution, θ_j = (π/4)(1 + GL5_NODES[j]).
const GL5_SIN2_THETA: [i128; 5] = [
    5_419_832_704,
    125_740_578_351,
    500_000_000_000,
    874_259_421_649,
    994_580_167_296,
];

/// sin(2θ_j) for the GL5 sin² substitution jacobian.
const GL5_SIN_2THETA: [i128; 5] = [
    146_839_478_582,
    663_113_520_619,
    1_000_000_000_000,
    663_113_520_619,
    146_839_478_582,
];

/// GL5 weights × π/4 (mapped to [0, π/2]).
const GL5_WEIGHTS_PI4: [i128; 5] = [
    186_081_940_383,
    375_914_078_760,
    446_804_288_511,
    375_914_078_760,
    186_081_940_383,
];

// ─── Gate 1: eigendecompose_2x2 ──────────────────────────────────────────────

/// Closed-form eigendecomposition of a symmetric 2×2 matrix.
///
/// Input: symmetric matrix `[[a, b], [b, d]]` with entries at SCALE.
///
/// Returns `(λ₁, λ₂, [u00, u01, u10, u11])` where:
/// - `λ₁ ≥ λ₂` are eigenvalues at SCALE
/// - `[u00, u01]` is the unit eigenvector for `λ₁` at SCALE
/// - `[u10, u11]` is the unit eigenvector for `λ₂` at SCALE
///
/// Formula: `λ = ½(a+d) ± ½√((a−d)² + 4b²)`.
pub fn eigendecompose_2x2(
    a: i128,
    b: i128,
    d: i128,
) -> Result<(i128, i128, [i128; 4]), SolMathError> {
    let trace = a + d;
    let diff = a - d;

    let diff_sq = fp_mul_i(diff, diff)?;
    let four_b_sq = 4 * fp_mul_i(b, b)?;
    let disc_sq = diff_sq + four_b_sq;

    // disc = √((a-d)² + 4b²), always ≥ 0.
    let disc = if disc_sq <= 0 {
        0
    } else {
        fp_sqrt(disc_sq as u128)? as i128
    };

    let lam1 = (trace + disc) / 2;
    let lam2 = (trace - disc) / 2;

    // Eigenvectors. If b ≈ 0, the matrix is already diagonal.
    let b_abs = b.unsigned_abs();
    if b_abs <= 1000 {
        // Diagonal (or near-diagonal): eigenvectors are axis-aligned.
        if a >= d {
            Ok((lam1, lam2, [SCALE_I, 0, 0, SCALE_I]))
        } else {
            Ok((lam1, lam2, [0, SCALE_I, SCALE_I, 0]))
        }
    } else {
        // General case: eigenvector for λ₁ is (b, λ₁ − a), normalized.
        let v0 = b;
        let v1 = lam1 - a;
        let norm_sq = fp_mul_i(v0, v0)? + fp_mul_i(v1, v1)?;
        if norm_sq <= 0 {
            return Err(SolMathError::DegenerateVariance);
        }
        let norm = fp_sqrt(norm_sq as u128)? as i128;
        if norm == 0 {
            return Err(SolMathError::DegenerateVariance);
        }
        let u00 = fp_div_i(v0, norm)?;
        let u01 = fp_div_i(v1, norm)?;
        // Eigenvector for λ₂ is orthogonal: (-u01, u00).
        Ok((lam1, lam2, [u00, u01, -u01, u00]))
    }
}

// ─── Gate 2: chi2_call ───────────────────────────────────────────────────────

/// Non-central χ²(1) call price: `E[max(λ/2·(W+δ)² − K, 0)]` where `W ~ N(0,1)`.
///
/// All inputs at SCALE. Returns the expected payoff at SCALE.
///
/// - If `lambda2 ≈ 0`: returns `max(-k_eff, 0)`.
/// - If `k_eff ≤ 0` (always ITM): returns `½λ(1+δ²) − K` (no Φ calls).
/// - If `k_eff > 0`: uses the 4Φ + 2φ formula.
pub fn chi2_call(lambda2: i128, delta2: i128, k_eff: i128) -> Result<i128, SolMathError> {
    // Guard: λ ≈ 0 → degenerate, just the intrinsic value.
    if lambda2.abs() < 1000 {
        return Ok((-k_eff).max(0));
    }

    let delta_sq = fp_mul_i(delta2, delta2)?;
    let one_plus_delta_sq = SCALE_I + delta_sq;
    let half_lam = lambda2 / 2;
    let mean = fp_mul_i(half_lam, one_plus_delta_sq)?; // ½λ(1+δ²)

    // Branch 1: K ≤ 0 — always ITM, no Φ calls needed.
    if k_eff <= 0 {
        return Ok(mean - k_eff);
    }

    // Branch 2: K > 0 — use 4Φ + 2φ formula.
    // τ = √(2K/λ)
    let two_k_over_lam = fp_div_i(2 * k_eff, lambda2)?;
    if two_k_over_lam <= 0 {
        return Ok((mean - k_eff).max(0));
    }
    let tau = fp_sqrt(two_k_over_lam as u128)? as i128;

    // P_tail = Φ(δ − τ) + Φ(−δ − τ)
    let p_a = norm_cdf_poly(delta2 - tau)?;
    let p_b = norm_cdf_poly(-delta2 - tau)?;
    let p_tail = p_a + p_b;

    // φ(τ − δ) and φ(τ + δ)
    let phi_a = norm_pdf(tau - delta2)?;
    let phi_b = norm_pdf(tau + delta2)?;

    // term1 = (½λ(1+δ²) − K) × P_tail
    let mean_minus_k = mean - k_eff;
    let term1 = fp_mul_i(mean_minus_k, p_tail)?;

    // term2 = ½λ × ((τ+δ)φ(τ−δ) + (τ−δ)φ(τ+δ))
    let inner = fp_mul_i(tau + delta2, phi_a)? + fp_mul_i(tau - delta2, phi_b)?;
    let term2 = fp_mul_i(half_lam, inner)?;

    Ok((term1 + term2).max(0))
}

// ─── Gate 3: analytical split ────────────────────────────────────────────────

/// GL7 integration of the tail region (Case 1: always-ITM inner integral).
///
/// Integrand: `(½λ₂(1+δ₂²) + ½λ₁(w₁+δ₁)² + q₀ − strike) × φ(w₁)`.
fn gl7_tail(
    lam1: i128,
    lam2: i128,
    del1: i128,
    del2: i128,
    q0: i128,
    strike: i128,
    lo: i128,
    hi: i128,
) -> Result<i128, SolMathError> {
    if hi <= lo {
        return Ok(0);
    }

    let half_width = (hi - lo) / 2;
    let midpoint = (hi + lo) / 2;

    let del2_sq = fp_mul_i(del2, del2)?;
    let inner_const = fp_mul_i(lam2 / 2, SCALE_I + del2_sq)?; // ½λ₂(1+δ₂²)

    let mut sum = 0i128;
    for j in 0..7 {
        let w1 = fp_mul_i(half_width, GL7_NODES[j])? + midpoint;
        let wt = fp_mul_i(half_width, GL7_WEIGHTS[j])?;

        let w1_d = w1 + del1;
        let outer_q = fp_mul_i(lam1 / 2, fp_mul_i(w1_d, w1_d)?)?;
        let integrand = inner_const + outer_q + q0 - strike;

        let phi = norm_pdf(w1)?;
        sum += fp_mul_i(wt, fp_mul_i(integrand, phi)?)?;
    }
    Ok(sum)
}

/// GL7 integration of the middle region using the sin² substitution.
///
/// `w₁ = wL + Δ·sin²(θ)`, `θ ∈ [0, π/2]`.
/// Uses precomputed sin²(θⱼ) and sin(2θⱼ) to avoid runtime trig.
fn gl7_middle_sin2(
    lam1: i128,
    lam2: i128,
    del1: i128,
    del2: i128,
    q0: i128,
    strike: i128,
    wl: i128,
    delta_w: i128,
) -> Result<i128, SolMathError> {
    if delta_w <= 0 {
        return Ok(0);
    }

    let mut sum = 0i128;
    for j in 0..7 {
        // w₁ = wL + Δ·sin²(θⱼ)
        let w1 = wl + fp_mul_i(delta_w, GL7_SIN2_THETA[j])?;
        // jacobian = Δ·sin(2θⱼ)
        let jacobian = fp_mul_i(delta_w, GL7_SIN_2THETA[j])?;

        // K_eff = strike − (½λ₁(w₁+δ₁)² + q₀)
        let w1_d = w1 + del1;
        let c_w1 = fp_mul_i(lam1 / 2, fp_mul_i(w1_d, w1_d)?)? + q0;
        let k_eff = strike - c_w1;

        // In the middle region K_eff should be ≥ 0, but clamp for safety.
        let val = chi2_call(lam2, del2, k_eff.max(0))?;

        let phi = norm_pdf(w1)?;
        // weight × val × φ(w₁) × jacobian
        let term = fp_mul_i(GL7_WEIGHTS_PI4[j], fp_mul_i(val, fp_mul_i(phi, jacobian)?)?)?;
        sum += term;
    }
    Ok(sum)
}

/// Compute `E[(Q − strike)⁺]` via the three-region split method.
///
/// Uses GL7 for each region (left tail, sin² middle, right tail).
fn call_split_single(
    lam1: i128,
    lam2: i128,
    del1: i128,
    del2: i128,
    q0: i128,
    strike: i128,
) -> Result<i128, SolMathError> {
    let k_const = strike - q0;

    // If K_const ≤ 0: Q > strike always. E[(Q-strike)⁺] = E[Q] - strike.
    if k_const <= 0 {
        let del1_sq = fp_mul_i(del1, del1)?;
        let del2_sq = fp_mul_i(del2, del2)?;
        let eq =
            fp_mul_i(lam1 / 2, SCALE_I + del1_sq)? + fp_mul_i(lam2 / 2, SCALE_I + del2_sq)? + q0
                - strike;
        return Ok(eq);
    }

    // Guard: lam1 too small → R would be enormous.
    if lam1 < 1000 {
        // Degenerate: just use the chi2_call for the whole thing.
        let del2_sq = fp_mul_i(del2, del2)?;
        return Ok(fp_mul_i(lam2 / 2, SCALE_I + del2_sq)? + q0 - strike);
    }

    // R = √(2·K_const / λ₁)
    let r_sq = fp_div_i(2 * k_const, lam1)?;
    let r = fp_sqrt(r_sq as u128)? as i128;

    let wl = -del1 - r;
    let wr = -del1 + r;

    // Left tail: [w_lo, wL]
    let w_lo = (wl - 6 * SCALE_I).min(-8 * SCALE_I);
    let i_left = gl7_tail(lam1, lam2, del1, del2, q0, strike, w_lo, wl)?;

    // Right tail: [wR, w_hi]
    let w_hi = (wr + 6 * SCALE_I).max(8 * SCALE_I);
    let i_right = gl7_tail(lam1, lam2, del1, del2, q0, strike, wr, w_hi)?;

    // Middle: sin² substitution on [wL, wR]
    let delta_w = wr - wl; // = 2R
    let i_middle = gl7_middle_sin2(lam1, lam2, del1, del2, q0, strike, wl, delta_w)?;

    Ok(i_left + i_middle + i_right)
}

/// Compute the analytical spread `E[(Q−d)⁺] − E[(Q−c)⁺]` via split method.
///
/// All inputs at SCALE. Returns the spread value at SCALE.
pub fn spread_split(
    lam1: i128,
    lam2: i128,
    del1: i128,
    del2: i128,
    q0: i128,
    d: i128,
    c: i128,
) -> Result<i128, SolMathError> {
    let call_d = call_split_single(lam1, lam2, del1, del2, q0, d)?;
    let call_c = call_split_single(lam1, lam2, del1, del2, q0, c)?;
    Ok(call_d - call_c)
}

/// √2 × GH3_NODES, pre-scaled.
const SQRT2_GH3: [i128; 3] = [
    -1_732_050_807_569, // -√2 × 1.224744871392
    0,
    1_732_050_807_569, //  √2 × 1.224744871392
];

/// Analytical spread with W₃ nesting: GL7 (sin² split) over W₁ × GH3 over W₃.
///
/// Q = ½λ₁(W₁+δ₁)² + ½λ₂(W₂+δ₂)² + ½λ₃(W₃+δ₃)² + q₀
/// Integrate W₃ via GH3, W₁ via GL7 three-region split, W₂ via chi2_call.
///
/// Returns E[(Q−d)⁺] − E[(Q−c)⁺].
fn spread_split_3d(
    lam1: i128,
    lam2: i128,
    lam3: i128,
    del1: i128,
    del2: i128,
    del3: i128,
    q0: i128,
    d: i128,
    c: i128,
) -> Result<i128, SolMathError> {
    // If λ₃ ≈ 0, fall back to 2D spread_split (no W₃ nesting needed).
    if lam3.abs() < 1000 {
        return spread_split(lam1, lam2, del1, del2, q0, d, c);
    }

    // GH3 outer loop over W₃ (physicist convention: ∫f(x)e^{-x²}dx ≈ Σ wₖ f(xₖ))
    // Standard normal: (1/√π) Σ wₖ f(√2 xₖ)
    let mut total = 0i128;
    for m in 0..3 {
        // W₃ = √2 × GH3_NODES[m] (standardised normal sample)
        let w3 = SQRT2_GH3[m];
        let w3_d = w3 + del3;

        // q₀_shifted = q₀ + ½λ₃(W₃+δ₃)²
        let q0_shifted = q0 + fp_mul_i(lam3 / 2, fp_mul_i(w3_d, w3_d)?)?;

        // Inner 2D spread: GL7 over W₁ × chi2_call over W₂, with shifted q₀
        let inner = spread_split(lam1, lam2, del1, del2, q0_shifted, d, c)?;

        // Weight: GH3_WEIGHTS[m] / √π
        let wt = fp_mul_i(GH3_WEIGHTS[m], INV_SQRT_PI)?;
        total += fp_mul_i(wt, inner)?;

        log_cu_marker(); // ── CU marker 5a/5b/5c: after GH3 W₃ node m ──
    }
    Ok(total)
}

// ─── GL5 variants for Tier 2 analytical ─────────────────────────────────────

/// GL5 tail integration (always ITM, GL5 quadrature).
fn gl5_tail(
    lam1: i128,
    lam2: i128,
    del1: i128,
    del2: i128,
    q0: i128,
    strike: i128,
    lo: i128,
    hi: i128,
) -> Result<i128, SolMathError> {
    if hi <= lo {
        return Ok(0);
    }
    let hw = (hi - lo) / 2;
    let mid = (hi + lo) / 2;
    let d2sq = fp_mul_i(del2, del2)?;
    let ic = fp_mul_i(lam2 / 2, SCALE_I + d2sq)?;
    let mut sum = 0i128;
    for j in 0..5 {
        let w1 = fp_mul_i(hw, GL5_NODES[j])? + mid;
        let wt = fp_mul_i(hw, GL5_WEIGHTS[j])?;
        let w1d = w1 + del1;
        let oq = fp_mul_i(lam1 / 2, fp_mul_i(w1d, w1d)?)?;
        let integ = ic + oq + q0 - strike;
        let phi = norm_pdf(w1)?;
        sum += fp_mul_i(wt, fp_mul_i(integ, phi)?)?;
    }
    Ok(sum)
}

/// GL5 middle integration with sin² substitution.
fn gl5_middle_sin2(
    lam1: i128,
    lam2: i128,
    del1: i128,
    del2: i128,
    q0: i128,
    strike: i128,
    wl: i128,
    delta_w: i128,
) -> Result<i128, SolMathError> {
    if delta_w <= 0 {
        return Ok(0);
    }
    let mut sum = 0i128;
    for j in 0..5 {
        let w1 = wl + fp_mul_i(delta_w, GL5_SIN2_THETA[j])?;
        let jac = fp_mul_i(delta_w, GL5_SIN_2THETA[j])?;
        let w1d = w1 + del1;
        let c_w1 = fp_mul_i(lam1 / 2, fp_mul_i(w1d, w1d)?)? + q0;
        let k_eff = strike - c_w1;
        let val = chi2_call(lam2, del2, k_eff.max(0))?;
        let phi = norm_pdf(w1)?;
        sum += fp_mul_i(GL5_WEIGHTS_PI4[j], fp_mul_i(val, fp_mul_i(phi, jac)?)?)?;
    }
    Ok(sum)
}

fn call_split_single_gl5(
    lam1: i128,
    lam2: i128,
    del1: i128,
    del2: i128,
    q0: i128,
    strike: i128,
) -> Result<i128, SolMathError> {
    let kc = strike - q0;
    if kc <= 0 {
        let d1sq = fp_mul_i(del1, del1)?;
        let d2sq = fp_mul_i(del2, del2)?;
        return Ok(
            fp_mul_i(lam1 / 2, SCALE_I + d1sq)? + fp_mul_i(lam2 / 2, SCALE_I + d2sq)? + q0 - strike,
        );
    }
    if lam1 < 1000 {
        let d2sq = fp_mul_i(del2, del2)?;
        return Ok(fp_mul_i(lam2 / 2, SCALE_I + d2sq)? + q0 - strike);
    }
    let rsq = fp_div_i(2 * kc, lam1)?;
    let r = fp_sqrt(rsq as u128)? as i128;
    let wl = -del1 - r;
    let wr = -del1 + r;
    let w_lo = (wl - 6 * SCALE_I).min(-8 * SCALE_I);
    let w_hi = (wr + 6 * SCALE_I).max(8 * SCALE_I);
    let il = gl7_tail(lam1, lam2, del1, del2, q0, strike, w_lo, wl)?;
    let ir = gl7_tail(lam1, lam2, del1, del2, q0, strike, wr, w_hi)?;
    let im = gl5_middle_sin2(lam1, lam2, del1, del2, q0, strike, wl, wr - wl)?;
    Ok(il + im + ir)
}

/// 2D spread with GL5 middle/tail.
pub fn spread_split_gl5(
    lam1: i128,
    lam2: i128,
    del1: i128,
    del2: i128,
    q0: i128,
    d: i128,
    c: i128,
) -> Result<i128, SolMathError> {
    Ok(call_split_single_gl5(lam1, lam2, del1, del2, q0, d)?
        - call_split_single_gl5(lam1, lam2, del1, del2, q0, c)?)
}

/// 3D spread with GH3 over W₃ and GL5 middle. For Tier 2 tx2.
pub fn spread_split_3d_gl5(
    lam1: i128,
    lam2: i128,
    lam3: i128,
    del1: i128,
    del2: i128,
    del3: i128,
    q0: i128,
    d: i128,
    c: i128,
) -> Result<i128, SolMathError> {
    if lam3.abs() < 1000 {
        return spread_split_gl5(lam1, lam2, del1, del2, q0, d, c);
    }
    let mut total = 0i128;
    for m in 0..3 {
        let w3 = SQRT2_GH3[m];
        let w3d = w3 + del3;
        let q0s = q0 + fp_mul_i(lam3 / 2, fp_mul_i(w3d, w3d)?)?;
        let inner = spread_split_gl5(lam1, lam2, del1, del2, q0s, d, c)?;
        let wt = fp_mul_i(GH3_WEIGHTS[m], INV_SQRT_PI)?;
        total += fp_mul_i(wt, inner)?;
    }
    Ok(total)
}

// ─── Gates 4–5: full engine ──────────────────────────────────────────────────

/// All precomputed values from tx1, consumed by tx2.
#[derive(Clone)]
pub struct N3CvSetup {
    /// Precomputed exp(aᵢ·√2·GH5_NODESⱼ), 3×5 values.
    pub exp_az: [[i128; 5]; 3],
    /// Precomputed exp(bᵢ·√2·GH5_NODESₖ), 3×5 values.
    pub exp_bz: [[i128; 5]; 3],
    /// Conditional mean slope: Σw / var_S.
    pub c_vec: [i128; 3],
    /// Risk-neutral drift: μᵢ = −σᵢ²T/2.
    pub mu: [i128; 3],
    /// μ_S = wᵀμ.
    pub mu_s: i128,
    /// σ_S = √(wᵀΣw).
    pub sigma_s: i128,
    /// var_S = wᵀΣw.
    pub var_s: i128,
    /// Volatile weights.
    pub w_vol: [i128; 3],
    /// Stablecoin weight.
    pub w_stable: i128,
    /// L_inner column 0 (a coefficients).
    pub a_coeff: [i128; 3],
    /// L_inner column 1 (b coefficients).
    pub b_coeff: [i128; 3],
    /// Analytical result: E[(Q−d)⁺] − E[(Q−c)⁺].
    pub analytical: i128,
    /// Deductible.
    pub d: i128,
    /// Cap.
    pub c: i128,
    /// Eigendecomposition data for Q_model in correction loop.
    pub eigen: EigenFull,
}

/// Reconstruct N3CvSetup from serialised PDA fields (for tx2 correction only).
///
/// The `eigen` field is set to a dummy — it's unused by `n3_cv_correction`
/// which computes Q_full = ½[Σ wᵢYᵢ² − s²] directly.
pub fn n3_cv_setup_from_pda(
    exp_az: [[i128; 5]; 3],
    exp_bz: [[i128; 5]; 3],
    c_vec: [i128; 3],
    mu: [i128; 3],
    mu_s: i128,
    sigma_s: i128,
    var_s: i128,
    w_vol: [i128; 3],
    w_stable: i128,
    a_coeff: [i128; 3],
    b_coeff: [i128; 3],
    analytical: i128,
    d: i128,
    c: i128,
) -> N3CvSetup {
    N3CvSetup {
        exp_az,
        exp_bz,
        c_vec,
        mu,
        mu_s,
        sigma_s,
        var_s,
        w_vol,
        w_stable,
        a_coeff,
        b_coeff,
        analytical,
        d,
        c,
        eigen: EigenFull {
            lam1: 0,
            lam2: 0,
            lam3: 0,
            del1: 0,
            del2: 0,
            del3: 0,
            q0: 0,
        },
    }
}

// ── Helpers: small fixed-point linear algebra ────────────────────────────────

fn dot3(a: &[i128; 3], b: &[i128; 3]) -> Result<i128, SolMathError> {
    Ok(fp_mul_i(a[0], b[0])? + fp_mul_i(a[1], b[1])? + fp_mul_i(a[2], b[2])?)
}

fn normalize3(v: [i128; 3]) -> Result<[i128; 3], SolMathError> {
    let nsq = dot3(&v, &v)?;
    if nsq <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let n = fp_sqrt(nsq as u128)? as i128;
    if n == 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    Ok([fp_div_i(v[0], n)?, fp_div_i(v[1], n)?, fp_div_i(v[2], n)?])
}

fn cross3(a: &[i128; 3], b: &[i128; 3]) -> Result<[i128; 3], SolMathError> {
    Ok([
        fp_mul_i(a[1], b[2])? - fp_mul_i(a[2], b[1])?,
        fp_mul_i(a[2], b[0])? - fp_mul_i(a[0], b[2])?,
        fp_mul_i(a[0], b[1])? - fp_mul_i(a[1], b[0])?,
    ])
}

/// QR basis: two columns orthogonal to w.
fn qr_basis(w: &[i128; 3]) -> Result<[[i128; 3]; 2], SolMathError> {
    let w_hat = normalize3(*w)?;

    // Pick seed axis = argmin(|w_hat[i]|)
    let mut axis = 0;
    let mut min_abs = w_hat[0].abs();
    for i in 1..3 {
        if w_hat[i].abs() < min_abs {
            min_abs = w_hat[i].abs();
            axis = i;
        }
    }

    let mut seed = [0i128; 3];
    seed[axis] = SCALE_I;

    // q1 = seed − (seed · w_hat) w_hat, normalized
    let sdw = dot3(&seed, &w_hat)?;
    let q1_raw = [
        seed[0] - fp_mul_i(sdw, w_hat[0])?,
        seed[1] - fp_mul_i(sdw, w_hat[1])?,
        seed[2] - fp_mul_i(sdw, w_hat[2])?,
    ];
    let q1 = normalize3(q1_raw)?;

    // q2 = w_hat × q1, normalized
    let q2_raw = cross3(&w_hat, &q1)?;
    let q2 = normalize3(q2_raw)?;

    Ok([q1, q2])
}

/// 3×3 Cholesky decomposition (lower triangular). Returns L such that L Lᵀ = A.
fn cholesky_3x3(a: &[[i128; 3]; 3]) -> Result<[[i128; 3]; 3], SolMathError> {
    let mut l = [[0i128; 3]; 3];

    // L[0][0] = √A[0][0]
    if a[0][0] <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    l[0][0] = fp_sqrt(a[0][0] as u128)? as i128;

    // L[1][0] = A[1][0] / L[0][0]
    l[1][0] = fp_div_i(a[1][0], l[0][0])?;

    // L[1][1] = √(A[1][1] − L[1][0]²)
    let tmp = a[1][1] - fp_mul_i(l[1][0], l[1][0])?;
    if tmp <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    l[1][1] = fp_sqrt(tmp as u128)? as i128;

    // L[2][0] = A[2][0] / L[0][0]
    l[2][0] = fp_div_i(a[2][0], l[0][0])?;

    // L[2][1] = (A[2][1] − L[2][0] L[1][0]) / L[1][1]
    l[2][1] = fp_div_i(a[2][1] - fp_mul_i(l[2][0], l[1][0])?, l[1][1])?;

    // L[2][2] = √(A[2][2] − L[2][0]² − L[2][1]²)
    let tmp = a[2][2] - fp_mul_i(l[2][0], l[2][0])? - fp_mul_i(l[2][1], l[2][1])?;
    if tmp <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    l[2][2] = fp_sqrt(tmp as u128)? as i128;

    Ok(l)
}

/// Multiply Lᵀ × v where L is 3×3 lower triangular.
fn lt_mul_vec(l: &[[i128; 3]; 3], v: &[i128; 3]) -> Result<[i128; 3], SolMathError> {
    Ok([
        fp_mul_i(l[0][0], v[0])? + fp_mul_i(l[1][0], v[1])? + fp_mul_i(l[2][0], v[2])?,
        fp_mul_i(l[1][1], v[1])? + fp_mul_i(l[2][1], v[2])?,
        fp_mul_i(l[2][2], v[2])?,
    ])
}

/// π/2 at SCALE.
const PI_HALF: i128 = 1_570_796_326_795;

/// π at SCALE.
const PI_SCALE: i128 = 3_141_592_653_590;

/// 2π/3 at SCALE.
const TWO_PI_OVER_3: i128 = 2_094_395_102_393;

/// Minimax polynomial coefficients for asin(x)/x = P(x²) on x² ∈ [0, 0.25].
/// Degree 7 in u = x². Max error in acos: 46 ULP (relative ~4.6e-11).
const ASIN_P: [i128; 8] = [
    999_999_999_963, // c₀ = 1.0
    166_666_677_102, // c₁ = 1/6
    74_999_283_356,  // c₂
    44_663_356_068,  // c₃
    30_084_954_404,  // c₄
    24_719_715_388,  // c₅
    7_281_064_563,   // c₆
    34_807_215_771,  // c₇
];

/// acos(y) via range-reduced minimax polynomial. Input y at SCALE, |y| ≤ SCALE.
///
/// - |y| ≤ SCALE/2: `acos(y) = π/2 − y·P(y²)`
/// - y > SCALE/2:   `acos(y) = 2·√((1−y)/2)·P((1−y)/2)`
/// - y < −SCALE/2:  `acos(y) = π − 2·√((1+y)/2)·P((1+y)/2)`
///
/// where P(u) = asin(√u)/√u is a degree-7 minimax polynomial on [0, 0.25].
/// Cost: ~8 fp_mul (Horner) + 0-1 fp_sqrt ≈ 2K CU. Max error: 46 ULP.
fn acos_fixed(y: i128) -> Result<i128, SolMathError> {
    if y >= SCALE_I {
        return Ok(0);
    }
    if y <= -SCALE_I {
        return Ok(PI_SCALE);
    }

    let half_scale = SCALE_I / 2;

    if y.abs() <= half_scale {
        // |y| ≤ 0.5: acos(y) = π/2 − asin(y), asin(y) = y·P(y²)
        let u = fp_mul_i(y, y)?;
        let p = horner8(&ASIN_P, u);
        let asin_y = fp_mul_i(y, p)?;
        Ok(PI_HALF - asin_y)
    } else if y > 0 {
        // y > 0.5: acos(y) = 2·√((1−y)/2)·P((1−y)/2)
        let u = (SCALE_I - y) / 2; // u ∈ [0, SCALE/4], at SCALE
        let t = fp_sqrt(u as u128)? as i128; // √u at SCALE
        let p = horner8(&ASIN_P, u);
        Ok(2 * fp_mul_i(t, p)?)
    } else {
        // y < −0.5: acos(y) = π − 2·√((1+y)/2)·P((1+y)/2)
        let u = (SCALE_I + y) / 2;
        let t = fp_sqrt(u as u128)? as i128;
        let p = horner8(&ASIN_P, u);
        Ok(PI_SCALE - 2 * fp_mul_i(t, p)?)
    }
}

/// Horner evaluation of 8-term polynomial (ascending coefficients) at u (SCALE).
#[inline]
fn horner8(c: &[i128; 8], u: i128) -> i128 {
    let mut r = c[7];
    r = r * u / SCALE_I + c[6];
    r = r * u / SCALE_I + c[5];
    r = r * u / SCALE_I + c[4];
    r = r * u / SCALE_I + c[3];
    r = r * u / SCALE_I + c[2];
    r = r * u / SCALE_I + c[1];
    r = r * u / SCALE_I + c[0];
    r
}

/// 3×3 symmetric eigendecomposition via Cardano's trigonometric method.
///
/// All arithmetic in fixed-point at SCALE. Returns `(eigenvalues_desc, eigenvectors)`.
/// Uses `fp_sqrt`, `acos_fixed`, `cos_fixed` — no floating point, no dependencies.
fn eigendecompose_3x3_sym(a: &[[i128; 3]; 3]) -> Result<([i128; 3], [[i128; 3]; 3]), SolMathError> {
    // trace / 3
    let tr = a[0][0] + a[1][1] + a[2][2];
    let q = tr / 3; // at SCALE

    // p² = (1/6) Σ (A[i][i] - q)² + 2 Σ A[i][j]²  (off-diagonal i<j)
    let d0 = a[0][0] - q;
    let d1 = a[1][1] - q;
    let d2 = a[2][2] - q;
    let p2_6 = fp_mul_i(d0, d0)?
        + fp_mul_i(d1, d1)?
        + fp_mul_i(d2, d2)?
        + 2 * (fp_mul_i(a[0][1], a[0][1])?
            + fp_mul_i(a[0][2], a[0][2])?
            + fp_mul_i(a[1][2], a[1][2])?);

    if p2_6 < 100 {
        // Near-diagonal: eigenvalues ≈ diagonal entries
        let mut evals = [a[0][0], a[1][1], a[2][2]];
        if evals[0] < evals[1] {
            evals.swap(0, 1);
        }
        if evals[1] < evals[2] {
            evals.swap(1, 2);
        }
        if evals[0] < evals[1] {
            evals.swap(0, 1);
        }
        let mut evecs = [[0i128; 3]; 3];
        evecs[0][0] = SCALE_I;
        evecs[1][1] = SCALE_I;
        evecs[2][2] = SCALE_I;
        return Ok((evals, evecs));
    }

    // p = √(p²/6).  p2_6 is at SCALE (it's fp_mul results), so p = √(p2_6/6).
    // p2_6 / 6 is at SCALE. fp_sqrt expects SCALE-valued input, returns SCALE-valued.
    let p2_div6 = p2_6 / 6;
    let p = if p2_div6 > 0 {
        fp_sqrt(p2_div6 as u128)? as i128
    } else {
        0
    };

    if p < 100 {
        let mut evals = [a[0][0], a[1][1], a[2][2]];
        if evals[0] < evals[1] {
            evals.swap(0, 1);
        }
        if evals[1] < evals[2] {
            evals.swap(1, 2);
        }
        if evals[0] < evals[1] {
            evals.swap(0, 1);
        }
        let mut evecs = [[0i128; 3]; 3];
        evecs[0][0] = SCALE_I;
        evecs[1][1] = SCALE_I;
        evecs[2][2] = SCALE_I;
        return Ok((evals, evecs));
    }

    // B = (1/p)(A - qI), at SCALE
    let b00 = fp_div_i(d0, p)?;
    let b11 = fp_div_i(d1, p)?;
    let b22 = fp_div_i(d2, p)?;
    let b01 = fp_div_i(a[0][1], p)?;
    let b02 = fp_div_i(a[0][2], p)?;
    let b12 = fp_div_i(a[1][2], p)?;

    // det(B) / 2 at SCALE
    // det(B) = b00(b11 b22 - b12²) - b01(b01 b22 - b12 b02) + b02(b01 b12 - b11 b02)
    let det_b = fp_mul_i(b00, fp_mul_i(b11, b22)? - fp_mul_i(b12, b12)?)?
        - fp_mul_i(b01, fp_mul_i(b01, b22)? - fp_mul_i(b12, b02)?)?
        + fp_mul_i(b02, fp_mul_i(b01, b12)? - fp_mul_i(b11, b02)?)?;
    let r = det_b / 2; // at SCALE

    // φ = acos(r) / 3  (r is at SCALE, already in [-SCALE, SCALE] range)
    let r_clamped = r.max(-SCALE_I).min(SCALE_I);
    let phi = acos_fixed(r_clamped)? / 3;

    // Eigenvalues: q + 2p cos(φ), q + 2p cos(φ + 2π/3), q + 2p cos(φ - 2π/3)
    let two_p = 2 * p;
    let e1 = q + fp_mul_i(two_p, cos_fixed(phi)?)?;
    let e3 = q + fp_mul_i(two_p, cos_fixed(phi + TWO_PI_OVER_3)?)?;
    let e2 = tr - e1 - e3; // trace identity ensures precision

    // Sort descending
    let mut evals = [e1, e2, e3];
    if evals[0] < evals[1] {
        evals.swap(0, 1);
    }
    if evals[1] < evals[2] {
        evals.swap(1, 2);
    }
    if evals[0] < evals[1] {
        evals.swap(0, 1);
    }

    log_cu_marker(); // ── CU marker E1: after Cardano eigenvalues ──

    // Eigenvectors via (A - λI) null-space using cross products of rows.
    let mut evecs = [[0i128; 3]; 3];
    for k in 0..3 {
        let lk = evals[k];
        let r0 = [a[0][0] - lk, a[0][1], a[0][2]];
        let r1 = [a[1][0], a[1][1] - lk, a[1][2]];
        let r2 = [a[2][0], a[2][1], a[2][2] - lk];

        // Cross products of row pairs — pick the one with largest norm²
        let c01 = cross3(&r0, &r1)?;
        let c02 = cross3(&r0, &r2)?;
        let c12 = cross3(&r1, &r2)?;

        let n01 = dot3(&c01, &c01)?;
        let n02 = dot3(&c02, &c02)?;
        let n12 = dot3(&c12, &c12)?;

        let best = if n01 >= n02 && n01 >= n12 {
            c01
        } else if n02 >= n12 {
            c02
        } else {
            c12
        };

        let best_nsq = dot3(&best, &best)?;
        if best_nsq > 100 {
            evecs[k] = normalize3(best)?;
        } else {
            // Degenerate: use axis vector
            evecs[k] = [0; 3];
            evecs[k][k % 3] = SCALE_I;
        }
    }

    log_cu_marker(); // ── CU marker E2: after eigenvectors ──

    Ok((evals, evecs))
}

/// Data from full 3×3 eigendecomposition.
#[derive(Clone)]
pub struct EigenFull {
    pub lam1: i128,
    pub lam2: i128,
    pub lam3: i128,
    pub del1: i128,
    pub del2: i128,
    pub del3: i128,
    pub q0: i128,
}

/// Full 3×3 analytical eigendecomposition of Q = ½ Yᵀ M Y.
///
/// L_full = chol(Σ), A = Lᵀ M L, eigen(A). Top 2 eigenvalues for GL7 split,
/// λ₃ absorbed into q₀. Returns L_full and u₁, u₂ eigenvectors so the
/// correction loop can compute Q_model consistently.
fn analytical_eigen_full(
    sigma_mat: &[[i128; 3]; 3],
    w: &[i128; 3],
    mu: &[i128; 3],
) -> Result<EigenFull, SolMathError> {
    let l_full = cholesky_3x3(sigma_mat)?;

    let mu_s = fp_mul_i(w[0], mu[0])? + fp_mul_i(w[1], mu[1])? + fp_mul_i(w[2], mu[2])?;
    let m_mu = [
        fp_mul_i(w[0], mu[0] - mu_s)?,
        fp_mul_i(w[1], mu[1] - mu_s)?,
        fp_mul_i(w[2], mu[2] - mu_s)?,
    ];

    // M × L columns: (Mv)_i = w_i v_i − w_i (wᵀv)
    let mut ml = [[0i128; 3]; 3];
    for col in 0..3 {
        let l_col = [l_full[0][col], l_full[1][col], l_full[2][col]];
        let wt_v =
            fp_mul_i(w[0], l_col[0])? + fp_mul_i(w[1], l_col[1])? + fp_mul_i(w[2], l_col[2])?;
        for i in 0..3 {
            ml[i][col] = fp_mul_i(w[i], l_col[i])? - fp_mul_i(w[i], wt_v)?;
        }
    }

    // A = Lᵀ M L (3×3 symmetric)
    let mut a_mat = [[0i128; 3]; 3];
    for r in 0..3 {
        for c in r..3 {
            let mut s = 0i128;
            for i in 0..3 {
                s += fp_mul_i(l_full[i][r], ml[i][c])?;
            }
            a_mat[r][c] = s;
            a_mat[c][r] = s;
        }
    }

    log_cu_marker(); // ── CU marker E0: before eigendecompose_3x3_sym ──

    let (evals, evecs) = eigendecompose_3x3_sym(&a_mat)?;
    let lam1 = evals[0].max(0);
    let lam2 = evals[1].max(0);
    let lam3 = evals[2].max(0);

    // ── CU markers E1 and E2 are inside eigendecompose_3x3_sym ──

    let b_full = lt_mul_vec(&l_full, &m_mu)?;

    let mut cv = [0i128; 3];
    for k in 0..3 {
        cv[k] = fp_mul_i(evecs[k][0], b_full[0])?
            + fp_mul_i(evecs[k][1], b_full[1])?
            + fp_mul_i(evecs[k][2], b_full[2])?;
    }

    log_cu_marker(); // ── CU marker E3: after evec projection (δ computation starts) ──

    let del1 = if lam1 > 1000 {
        fp_div_i(cv[0], lam1)?
    } else {
        0
    };
    let del2 = if lam2 > 1000 {
        fp_div_i(cv[1], lam2)?
    } else {
        0
    };
    let del3 = if lam3 > 1000 {
        fp_div_i(cv[2], lam3)?
    } else {
        0
    };

    // q₀ = ½μᵀMμ − ½Σλₖδₖ²  (all 3 eigenvalues subtracted)
    let mu_m_mu = fp_mul_i(mu[0], m_mu[0])? + fp_mul_i(mu[1], m_mu[1])? + fp_mul_i(mu[2], m_mu[2])?;
    let q0 = mu_m_mu / 2
        - fp_mul_i(lam1, fp_mul_i(del1, del1)?)? / 2
        - fp_mul_i(lam2, fp_mul_i(del2, del2)?)? / 2
        - fp_mul_i(lam3, fp_mul_i(del3, del3)?)? / 2;

    Ok(EigenFull {
        lam1,
        lam2,
        lam3,
        del1,
        del2,
        del3,
        q0,
    })
}

/// Build covariance matrix Σᵢⱼ = ρᵢⱼ σᵢ σⱼ T_y.
fn build_cov(
    sigmas: &[i128; 3],
    t_y: i128,
    rho: &[[i128; 3]; 3],
) -> Result<[[i128; 3]; 3], SolMathError> {
    let mut sigma = [[0i128; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            sigma[i][j] = fp_mul_i(fp_mul_i(fp_mul_i(rho[i][j], sigmas[i])?, sigmas[j])?, t_y)?;
        }
    }
    Ok(sigma)
}

/// Multiply a 3×3 symmetric matrix by a 3-vector: result = M × v.
fn mat3_vec(m: &[[i128; 3]; 3], v: &[i128; 3]) -> Result<[i128; 3], SolMathError> {
    let mut out = [0i128; 3];
    for i in 0..3 {
        out[i] = fp_mul_i(m[i][0], v[0])? + fp_mul_i(m[i][1], v[1])? + fp_mul_i(m[i][2], v[2])?;
    }
    Ok(out)
}

/// Compute the full setup for N=3 CV premium (tx1 equivalent).
///
/// Inputs: volatile weights, stablecoin weight, annual vols, tenor in days,
/// correlation matrix, deductible, cap — all at SCALE.
/// CU profiling marker — active on Solana BPF only.
#[inline(always)]
#[allow(unsafe_code)]
fn log_cu_marker() {
    #[cfg(target_os = "solana")]
    {
        extern "C" {
            fn sol_log_compute_units_();
        }
        unsafe {
            sol_log_compute_units_();
        }
    }
}

pub fn n3_cv_setup(
    w_vol: [i128; 3],
    w_stable: i128,
    sigmas: [i128; 3],
    t_days: u32,
    rho: [[i128; 3]; 3],
    d: i128,
    c: i128,
) -> Result<N3CvSetup, SolMathError> {
    let t_y = SCALE_I * (t_days as i128) / 365;

    // 1. Covariance matrix Σ and drift μ
    let sigma_mat = build_cov(&sigmas, t_y, &rho)?;
    let mut mu = [0i128; 3];
    for i in 0..3 {
        let s2 = fp_mul_i(sigmas[i], sigmas[i])?;
        mu[i] = -fp_mul_i(s2, t_y)? / 2;
    }

    log_cu_marker(); // ── CU marker 1: after covariance + drift ──

    // 2. μ_S, var_S, σ_S
    let mu_s = fp_mul_i(w_vol[0], mu[0])? + fp_mul_i(w_vol[1], mu[1])? + fp_mul_i(w_vol[2], mu[2])?;

    let sigma_w = mat3_vec(&sigma_mat, &w_vol)?; // Σw
    let var_s = dot3(&w_vol, &sigma_w)?;
    if var_s <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let sigma_s = fp_sqrt(var_s as u128)? as i128;

    // 3. c_vec = Σw / var_S (conditional mean slope)
    let c_vec = [
        fp_div_i(sigma_w[0], var_s)?,
        fp_div_i(sigma_w[1], var_s)?,
        fp_div_i(sigma_w[2], var_s)?,
    ];

    // 4. QR basis Q_perp (3×2, orthogonal to w)
    let q_perp = qr_basis(&w_vol)?;

    // 5. V = Σ − Σw(Σw)ᵀ / var_S (conditional covariance)
    let mut v_mat = [[0i128; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            v_mat[i][j] = sigma_mat[i][j] - fp_div_i(fp_mul_i(sigma_w[i], sigma_w[j])?, var_s)?;
        }
    }

    // 6. V_proj = Q_perpᵀ V Q_perp (2×2)
    let vq = [mat3_vec(&v_mat, &q_perp[0])?, mat3_vec(&v_mat, &q_perp[1])?];
    let v_proj = [
        [dot3(&q_perp[0], &vq[0])?, dot3(&q_perp[0], &vq[1])?],
        [dot3(&q_perp[1], &vq[0])?, dot3(&q_perp[1], &vq[1])?],
    ];

    // 7. 2×2 Cholesky: L_proj
    if v_proj[0][0] <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let l00 = fp_sqrt(v_proj[0][0] as u128)? as i128;
    let l10 = fp_div_i(v_proj[1][0], l00)?;
    let l11_sq = v_proj[1][1] - fp_mul_i(l10, l10)?;
    let l11 = if l11_sq <= 0 {
        0
    } else {
        fp_sqrt(l11_sq as u128)? as i128
    };

    // 8. L_inner = Q_perp @ L_proj (3×2)
    let mut a_coeff = [0i128; 3];
    let mut b_coeff = [0i128; 3];
    for i in 0..3 {
        a_coeff[i] = fp_mul_i(q_perp[0][i], l00)? + fp_mul_i(q_perp[1][i], l10)?;
        b_coeff[i] = fp_mul_i(q_perp[1][i], l11)?;
    }

    log_cu_marker(); // ── CU marker 2: after QR/Cholesky/L_inner ──

    // 9. Exp precomputation: exp(aᵢ·√2·nodeⱼ) and exp(bᵢ·√2·nodeₖ)
    let mut exp_az = [[0i128; 5]; 3];
    let mut exp_bz = [[0i128; 5]; 3];
    for i in 0..3 {
        for j in 0..5 {
            exp_az[i][j] = exp_fixed_i(fp_mul_i(a_coeff[i], SQRT2_GH5[j])?)?;
            exp_bz[i][j] = exp_fixed_i(fp_mul_i(b_coeff[i], SQRT2_GH5[j])?)?;
        }
    }

    log_cu_marker(); // ── CU marker 3: after exp precomputation (30 exp calls) ──

    // 10. Full 3×3 eigendecomposition for analytical formula
    let ef = analytical_eigen_full(&sigma_mat, &w_vol, &mu)?;

    log_cu_marker(); // ── CU marker 4: after eigendecomp (Cardano + acos) ──

    // 11. Analytical spread (3D: GL7 over W₁ × GH3 over W₃ × chi2_call over W₂)
    let analytical = spread_split_3d(
        ef.lam1, ef.lam2, ef.lam3, ef.del1, ef.del2, ef.del3, ef.q0, d, c,
    )?;

    log_cu_marker(); // ── CU marker 6: after analytical spread (all 3 GH3 nodes done) ──

    Ok(N3CvSetup {
        exp_az,
        exp_bz,
        c_vec,
        mu,
        mu_s,
        sigma_s,
        var_s,
        w_vol,
        w_stable,
        a_coeff,
        b_coeff,
        analytical,
        d,
        c,
        eigen: ef,
    })
}

/// Setup + eigendecomp WITHOUT the analytical spread (for new 2-tx split).
///
/// Returns `N3CvSetup` with `analytical = 0`. The eigenvalues are in `setup.eigen`.
/// Caller runs `n3_cv_correction` in tx1, then `analytical_from_eigen` in tx2.
pub fn n3_cv_setup_no_spread(
    w_vol: [i128; 3],
    w_stable: i128,
    sigmas: [i128; 3],
    t_days: u32,
    rho: [[i128; 3]; 3],
    d: i128,
    c: i128,
) -> Result<N3CvSetup, SolMathError> {
    let t_y = SCALE_I * (t_days as i128) / 365;
    let sigma_mat = build_cov(&sigmas, t_y, &rho)?;
    let mut mu = [0i128; 3];
    for i in 0..3 {
        let s2 = fp_mul_i(sigmas[i], sigmas[i])?;
        mu[i] = -fp_mul_i(s2, t_y)? / 2;
    }
    let mu_s = fp_mul_i(w_vol[0], mu[0])? + fp_mul_i(w_vol[1], mu[1])? + fp_mul_i(w_vol[2], mu[2])?;
    let sigma_w = mat3_vec(&sigma_mat, &w_vol)?;
    let var_s = dot3(&w_vol, &sigma_w)?;
    if var_s <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let sigma_s = fp_sqrt(var_s as u128)? as i128;
    let c_vec = [
        fp_div_i(sigma_w[0], var_s)?,
        fp_div_i(sigma_w[1], var_s)?,
        fp_div_i(sigma_w[2], var_s)?,
    ];
    let q_perp = qr_basis(&w_vol)?;
    let mut v_mat = [[0i128; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            v_mat[i][j] = sigma_mat[i][j] - fp_div_i(fp_mul_i(sigma_w[i], sigma_w[j])?, var_s)?;
        }
    }
    let vq = [mat3_vec(&v_mat, &q_perp[0])?, mat3_vec(&v_mat, &q_perp[1])?];
    let v_proj = [
        [dot3(&q_perp[0], &vq[0])?, dot3(&q_perp[0], &vq[1])?],
        [dot3(&q_perp[1], &vq[0])?, dot3(&q_perp[1], &vq[1])?],
    ];
    if v_proj[0][0] <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let l00 = fp_sqrt(v_proj[0][0] as u128)? as i128;
    let l10 = fp_div_i(v_proj[1][0], l00)?;
    let l11_sq = v_proj[1][1] - fp_mul_i(l10, l10)?;
    let l11 = if l11_sq <= 0 {
        0
    } else {
        fp_sqrt(l11_sq as u128)? as i128
    };
    let mut a_coeff = [0i128; 3];
    let mut b_coeff = [0i128; 3];
    for i in 0..3 {
        a_coeff[i] = fp_mul_i(q_perp[0][i], l00)? + fp_mul_i(q_perp[1][i], l10)?;
        b_coeff[i] = fp_mul_i(q_perp[1][i], l11)?;
    }
    let mut exp_az = [[0i128; 5]; 3];
    let mut exp_bz = [[0i128; 5]; 3];
    for i in 0..3 {
        for j in 0..5 {
            exp_az[i][j] = exp_fixed_i(fp_mul_i(a_coeff[i], SQRT2_GH5[j])?)?;
            exp_bz[i][j] = exp_fixed_i(fp_mul_i(b_coeff[i], SQRT2_GH5[j])?)?;
        }
    }
    let ef = analytical_eigen_full(&sigma_mat, &w_vol, &mu)?;
    Ok(N3CvSetup {
        exp_az,
        exp_bz,
        c_vec,
        mu,
        mu_s,
        sigma_s,
        var_s,
        w_vol,
        w_stable,
        a_coeff,
        b_coeff,
        analytical: 0,
        d,
        c,
        eigen: ef,
    })
}

/// Compute the analytical spread from precomputed eigenvalues (for tx2 of new 2-tx split).
///
/// Runs spread_split_3d: GH3 over W₃ × GL7 × chi2_call.
pub fn analytical_from_eigen(ef: &EigenFull, d: i128, c: i128) -> Result<i128, SolMathError> {
    spread_split_3d(
        ef.lam1, ef.lam2, ef.lam3, ef.del1, ef.del2, ef.del3, ef.q0, d, c,
    )
}

// ─── 2-tx split: tx1 = setup + eigendecomp + Q_model correction ────────────

/// Result from tx1 of the 2D split.
pub struct N3Tx1Result {
    /// Q_model correction (IL − Q_model payoff difference).
    pub correction: i128,
    /// 2D eigenvalues for tx2 spread_split (λ₃ absorbed into q₀_2d).
    pub lam1: i128,
    pub lam2: i128,
    pub del1: i128,
    pub del2: i128,
    /// q₀ with λ₃ component absorbed: q₀ + ½λ₃(1+δ₃²).
    pub q0_2d: i128,
    pub d: i128,
    pub c: i128,
}

/// Forward-solve L x = b where L is 3×3 lower triangular. Returns x.
fn forward_sub(l: &[[i128; 3]; 3], b: &[i128; 3]) -> Result<[i128; 3], SolMathError> {
    let x0 = fp_div_i(b[0], l[0][0])?;
    let x1 = fp_div_i(b[1] - fp_mul_i(l[1][0], x0)?, l[1][1])?;
    let x2 = fp_div_i(
        b[2] - fp_mul_i(l[2][0], x0)? - fp_mul_i(l[2][1], x1)?,
        l[2][2],
    )?;
    Ok([x0, x1, x2])
}

/// Intermediate data from setup+eigendecomp, consumed by correction loop.
/// Large arrays are Box'd to stay under BPF's 4KB stack limit.
struct Tx1Intermediates {
    exp_az: alloc::boxed::Box<[[i128; 5]; 3]>,
    exp_bz: alloc::boxed::Box<[[i128; 5]; 3]>,
    exp_mu_tab: [i128; 3],
    exp_mu_s: i128,
    exp_gamma: alloc::boxed::Box<[[i128; 7]; 3]>,
    exp_sigma: [i128; 7],
    w_vol: [i128; 3],
    w_stable: i128,
    sigma_s: i128,
    mu_s: i128,
    lam1: i128,
    lam2: i128,
    del1: i128,
    del2: i128,
    q0: i128,
    q0_2d: i128,
    alpha_rate: [i128; 2],
    rz_a: [[i128; 5]; 2],
    rz_b: [[i128; 5]; 2],
    d: i128,
    c: i128,
}

/// Phase 1: setup + eigendecomp + exp precomp + R/α precomputation.
#[inline(never)]
fn tx1_phase1(
    w_vol: [i128; 3],
    w_stable: i128,
    sigmas: [i128; 3],
    t_days: u32,
    rho: [[i128; 3]; 3],
    d: i128,
    c: i128,
) -> Result<Tx1Intermediates, SolMathError> {
    let t_y = SCALE_I * (t_days as i128) / 365;
    let sigma_mat = build_cov(&sigmas, t_y, &rho)?;
    let mut mu = [0i128; 3];
    for i in 0..3 {
        mu[i] = -fp_mul_i(fp_mul_i(sigmas[i], sigmas[i])?, t_y)? / 2;
    }
    let mu_s = fp_mul_i(w_vol[0], mu[0])? + fp_mul_i(w_vol[1], mu[1])? + fp_mul_i(w_vol[2], mu[2])?;
    let sigma_w = mat3_vec(&sigma_mat, &w_vol)?;
    let var_s = dot3(&w_vol, &sigma_w)?;
    if var_s <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let sigma_s = fp_sqrt(var_s as u128)? as i128;
    let c_vec = [
        fp_div_i(sigma_w[0], var_s)?,
        fp_div_i(sigma_w[1], var_s)?,
        fp_div_i(sigma_w[2], var_s)?,
    ];

    let q_perp = qr_basis(&w_vol)?;
    let mut v_mat = [[0i128; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            v_mat[i][j] = sigma_mat[i][j] - fp_div_i(fp_mul_i(sigma_w[i], sigma_w[j])?, var_s)?;
        }
    }
    let vq = [mat3_vec(&v_mat, &q_perp[0])?, mat3_vec(&v_mat, &q_perp[1])?];
    let v_proj = [
        [dot3(&q_perp[0], &vq[0])?, dot3(&q_perp[0], &vq[1])?],
        [dot3(&q_perp[1], &vq[0])?, dot3(&q_perp[1], &vq[1])?],
    ];
    if v_proj[0][0] <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let l00 = fp_sqrt(v_proj[0][0] as u128)? as i128;
    let l10 = fp_div_i(v_proj[1][0], l00)?;
    let l11_sq = v_proj[1][1] - fp_mul_i(l10, l10)?;
    let l11 = if l11_sq <= 0 {
        0
    } else {
        fp_sqrt(l11_sq as u128)? as i128
    };
    let mut a_coeff = [0i128; 3];
    let mut b_coeff = [0i128; 3];
    for i in 0..3 {
        a_coeff[i] = fp_mul_i(q_perp[0][i], l00)? + fp_mul_i(q_perp[1][i], l10)?;
        b_coeff[i] = fp_mul_i(q_perp[1][i], l11)?;
    }

    // Exp precomp (reciprocal trick) — Box'd for BPF stack
    let mut exp_az = alloc::boxed::Box::new([[0i128; 5]; 3]);
    let mut exp_bz = alloc::boxed::Box::new([[0i128; 5]; 3]);
    for i in 0..3 {
        exp_az[i][2] = SCALE_I;
        exp_az[i][3] = exp_fixed_i(fp_mul_i(a_coeff[i], SQRT2_GH5[3])?)?;
        exp_az[i][4] = exp_fixed_i(fp_mul_i(a_coeff[i], SQRT2_GH5[4])?)?;
        exp_az[i][1] = SCALE_SQ / exp_az[i][3];
        exp_az[i][0] = SCALE_SQ / exp_az[i][4];
        exp_bz[i][2] = SCALE_I;
        exp_bz[i][3] = exp_fixed_i(fp_mul_i(b_coeff[i], SQRT2_GH5[3])?)?;
        exp_bz[i][4] = exp_fixed_i(fp_mul_i(b_coeff[i], SQRT2_GH5[4])?)?;
        exp_bz[i][1] = SCALE_SQ / exp_bz[i][3];
        exp_bz[i][0] = SCALE_SQ / exp_bz[i][4];
    }
    let exp_mu_tab = [
        exp_fixed_i(mu[0])?,
        exp_fixed_i(mu[1])?,
        exp_fixed_i(mu[2])?,
    ];
    let exp_mu_s = exp_fixed_i(mu_s)?;
    let gamma = [
        fp_mul_i(c_vec[0], sigma_s)?,
        fp_mul_i(c_vec[1], sigma_s)?,
        fp_mul_i(c_vec[2], sigma_s)?,
    ];
    let mut exp_gamma = alloc::boxed::Box::new([[0i128; 7]; 3]);
    let mut exp_sigma = [0i128; 7];
    exp_sigma[3] = SCALE_I;
    for m in 4..7 {
        exp_sigma[m] = exp_fixed_i(fp_mul_i(sigma_s, SQRT2_GH7[m])?)?;
    }
    for m in 0..3 {
        exp_sigma[m] = SCALE_SQ / exp_sigma[6 - m];
    }
    for i in 0..3 {
        exp_gamma[i][3] = SCALE_I;
        for m in 4..7 {
            exp_gamma[i][m] = exp_fixed_i(fp_mul_i(gamma[i], SQRT2_GH7[m])?)?;
        }
        for m in 0..3 {
            exp_gamma[i][m] = SCALE_SQ / exp_gamma[i][6 - m];
        }
    }

    // Eigendecomp
    let l_full = cholesky_3x3(&sigma_mat)?;
    let m_mu = [
        fp_mul_i(w_vol[0], mu[0] - mu_s)?,
        fp_mul_i(w_vol[1], mu[1] - mu_s)?,
        fp_mul_i(w_vol[2], mu[2] - mu_s)?,
    ];
    let mut ml = [[0i128; 3]; 3];
    for col in 0..3 {
        let l_col = [l_full[0][col], l_full[1][col], l_full[2][col]];
        let wt_v = fp_mul_i(w_vol[0], l_col[0])?
            + fp_mul_i(w_vol[1], l_col[1])?
            + fp_mul_i(w_vol[2], l_col[2])?;
        for i in 0..3 {
            ml[i][col] = fp_mul_i(w_vol[i], l_col[i])? - fp_mul_i(w_vol[i], wt_v)?;
        }
    }
    let mut a_mat = [[0i128; 3]; 3];
    for r in 0..3 {
        for cc in r..3 {
            let mut s = 0i128;
            for i in 0..3 {
                s += fp_mul_i(l_full[i][r], ml[i][cc])?;
            }
            a_mat[r][cc] = s;
            a_mat[cc][r] = s;
        }
    }
    let (evals, evecs) = eigendecompose_3x3_sym(&a_mat)?;
    let lam1 = evals[0].max(0);
    let lam2 = evals[1].max(0);
    let lam3 = evals[2].max(0);
    let b_full = lt_mul_vec(&l_full, &m_mu)?;
    let mut cv = [0i128; 3];
    for kk in 0..3 {
        cv[kk] = fp_mul_i(evecs[kk][0], b_full[0])?
            + fp_mul_i(evecs[kk][1], b_full[1])?
            + fp_mul_i(evecs[kk][2], b_full[2])?;
    }
    let del1 = if lam1 > 1000 {
        fp_div_i(cv[0], lam1)?
    } else {
        0
    };
    let del2 = if lam2 > 1000 {
        fp_div_i(cv[1], lam2)?
    } else {
        0
    };
    let del3 = if lam3 > 1000 {
        fp_div_i(cv[2], lam3)?
    } else {
        0
    };
    let mu_m_mu = fp_mul_i(mu[0], m_mu[0])? + fp_mul_i(mu[1], m_mu[1])? + fp_mul_i(mu[2], m_mu[2])?;
    let q0 = mu_m_mu / 2
        - fp_mul_i(lam1, fp_mul_i(del1, del1)?)? / 2
        - fp_mul_i(lam2, fp_mul_i(del2, del2)?)? / 2
        - fp_mul_i(lam3, fp_mul_i(del3, del3)?)? / 2;
    let q0_2d = q0 + fp_mul_i(lam3, SCALE_I + fp_mul_i(del3, del3)?)? / 2;

    // R = U^T L_full^{-1} L_inner, α_rate = U^T L_full^{-1} c_vec
    let linv_a = forward_sub(&l_full, &a_coeff)?;
    let linv_b = forward_sub(&l_full, &b_coeff)?;
    let linv_c = forward_sub(&l_full, &c_vec)?;
    let r00 = fp_mul_i(evecs[0][0], linv_a[0])?
        + fp_mul_i(evecs[0][1], linv_a[1])?
        + fp_mul_i(evecs[0][2], linv_a[2])?;
    let r01 = fp_mul_i(evecs[0][0], linv_b[0])?
        + fp_mul_i(evecs[0][1], linv_b[1])?
        + fp_mul_i(evecs[0][2], linv_b[2])?;
    let r10 = fp_mul_i(evecs[1][0], linv_a[0])?
        + fp_mul_i(evecs[1][1], linv_a[1])?
        + fp_mul_i(evecs[1][2], linv_a[2])?;
    let r11 = fp_mul_i(evecs[1][0], linv_b[0])?
        + fp_mul_i(evecs[1][1], linv_b[1])?
        + fp_mul_i(evecs[1][2], linv_b[2])?;
    let mut rz_a = [[0i128; 5]; 2];
    let mut rz_b = [[0i128; 5]; 2];
    for j in 0..5 {
        rz_a[0][j] = fp_mul_i(r00, SQRT2_GH5[j])?;
        rz_a[1][j] = fp_mul_i(r10, SQRT2_GH5[j])?;
        rz_b[0][j] = fp_mul_i(r01, SQRT2_GH5[j])?;
        rz_b[1][j] = fp_mul_i(r11, SQRT2_GH5[j])?;
    }
    let alpha_rate_0 = fp_mul_i(evecs[0][0], linv_c[0])?
        + fp_mul_i(evecs[0][1], linv_c[1])?
        + fp_mul_i(evecs[0][2], linv_c[2])?;
    let alpha_rate_1 = fp_mul_i(evecs[1][0], linv_c[0])?
        + fp_mul_i(evecs[1][1], linv_c[1])?
        + fp_mul_i(evecs[1][2], linv_c[2])?;

    Ok(Tx1Intermediates {
        exp_az,
        exp_bz,
        exp_mu_tab,
        exp_mu_s,
        exp_gamma,
        exp_sigma,
        w_vol,
        w_stable,
        sigma_s,
        mu_s,
        lam1,
        lam2,
        del1,
        del2,
        q0,
        q0_2d,
        alpha_rate: [alpha_rate_0, alpha_rate_1],
        rz_a,
        rz_b,
        d,
        c,
    })
}

// ── Power-of-2 shift arithmetic for the hot path ──

const SCALE_SHIFT: u32 = 40; // 2^40 ≈ 1.0995e12
const SCALE_P2: i128 = 1i128 << SCALE_SHIFT;

/// Fast fixed-point multiply using bit shift with rounding.
/// No overflow check, no Result — raw speed for the inner loop.
#[inline(always)]
fn fp_mul_s(a: i128, b: i128) -> i128 {
    (a * b + (1i128 << (SCALE_SHIFT - 1))) >> SCALE_SHIFT
}

/// Convert a value from SCALE (1e12) to SCALE_P2 (2^40).
#[inline(always)]
fn to_p2(val: i128) -> i128 {
    // val_p2 = val * SCALE_P2 / SCALE_I
    // Rewrite to avoid overflow: (val / SCALE_I) * SCALE_P2 + (val % SCALE_I) * SCALE_P2 / SCALE_I
    // But simpler: val * (SCALE_P2 / gcd) / (SCALE_I / gcd). Since both ~1e12, ratio ≈ 1.0995.
    // For values up to ~1e13 (typical): val * SCALE_P2 fits i128 (1e13 * 1e12 = 1e25 < 1.7e38).
    (val * SCALE_P2 + SCALE_I / 2) / SCALE_I
}

/// Convert a value from SCALE_P2 back to SCALE.
#[inline(always)]
fn from_p2(val: i128) -> i128 {
    (val * SCALE_I + SCALE_P2 / 2) / SCALE_P2
}

/// All P2-converted tables, heap-allocated to avoid BPF stack overflow.
struct P2Tables {
    eaz: [[i128; 5]; 3],
    ebz: [[i128; 5]; 3],
    egamma: [[i128; 7]; 3],
    esigma: [i128; 7],
    emu: [i128; 3],
    emu_s: i128,
    w: [i128; 3],
    ws: i128,
    sigma_s: i128,
    ar: [i128; 2],
    rza: [[i128; 5]; 2],
    rzb: [[i128; 5]; 2],
    lam1: i128,
    lam2: i128,
    del1: i128,
    del2: i128,
    q0: i128,
    d: i128,
    c: i128,
    ow: [i128; 7],
    iw: [i128; 25],
    s2gh7: [i128; 7],
}

/// Convert Tx1Intermediates to P2 scale, heap-allocated.
#[inline(never)]
fn build_p2_tables(t: &Tx1Intermediates) -> alloc::boxed::Box<P2Tables> {
    let mut p = alloc::boxed::Box::new(P2Tables {
        eaz: [[0; 5]; 3],
        ebz: [[0; 5]; 3],
        egamma: [[0; 7]; 3],
        esigma: [0; 7],
        emu: [0; 3],
        emu_s: to_p2(t.exp_mu_s),
        w: [to_p2(t.w_vol[0]), to_p2(t.w_vol[1]), to_p2(t.w_vol[2])],
        ws: to_p2(t.w_stable),
        sigma_s: to_p2(t.sigma_s),
        ar: [to_p2(t.alpha_rate[0]), to_p2(t.alpha_rate[1])],
        rza: [[0; 5]; 2],
        rzb: [[0; 5]; 2],
        lam1: to_p2(t.lam1),
        lam2: to_p2(t.lam2),
        del1: to_p2(t.del1),
        del2: to_p2(t.del2),
        q0: to_p2(t.q0),
        d: to_p2(t.d),
        c: to_p2(t.c),
        ow: [0; 7],
        iw: [0; 25],
        s2gh7: [0; 7],
    });
    for i in 0..3 {
        for j in 0..5 {
            p.eaz[i][j] = to_p2(t.exp_az[i][j]);
            p.ebz[i][j] = to_p2(t.exp_bz[i][j]);
        }
    }
    for i in 0..3 {
        p.emu[i] = to_p2(t.exp_mu_tab[i]);
    }
    for m in 0..7 {
        p.esigma[m] = to_p2(t.exp_sigma[m]);
        for i in 0..3 {
            p.egamma[i][m] = to_p2(t.exp_gamma[i][m]);
        }
    }
    for j in 0..5 {
        p.rza[0][j] = to_p2(t.rz_a[0][j]);
        p.rza[1][j] = to_p2(t.rz_a[1][j]);
        p.rzb[0][j] = to_p2(t.rz_b[0][j]);
        p.rzb[1][j] = to_p2(t.rz_b[1][j]);
    }
    for i in 0..7 {
        p.ow[i] = to_p2(OUTER_W[i]);
    }
    for j in 0..25 {
        p.iw[j] = to_p2(INNER_W_FLAT[j]);
    }
    for m in 0..7 {
        p.s2gh7[m] = to_p2(SQRT2_GH7[m]);
    }
    p
}

/// Phase 2: 7×5×5 Q_model correction loop using shift arithmetic.
#[inline(never)]
fn tx1_phase2(t: &Tx1Intermediates) -> Result<i128, SolMathError> {
    let p2 = build_p2_tables(t);

    let mut corr = 0i128;
    for i in 0..7 {
        let ds = fp_mul_s(p2.sigma_s, p2.s2gh7[i]);
        let p = fp_mul_s(p2.emu_s, p2.esigma[i]);
        let emc = [
            fp_mul_s(p2.emu[0], p2.egamma[0][i]),
            fp_mul_s(p2.emu[1], p2.egamma[1][i]),
            fp_mul_s(p2.emu[2], p2.egamma[2][i]),
        ];
        let a0 = fp_mul_s(ds, p2.ar[0]);
        let a1 = fp_mul_s(ds, p2.ar[1]);
        for j in 0..5 {
            let ra0j = p2.rza[0][j];
            let ra1j = p2.rza[1][j];
            for k in 0..5 {
                let ey0 = fp_mul_s(fp_mul_s(emc[0], p2.eaz[0][j]), p2.ebz[0][k]);
                let ey1 = fp_mul_s(fp_mul_s(emc[1], p2.eaz[1][j]), p2.ebz[1][k]);
                let ey2 = fp_mul_s(fp_mul_s(emc[2], p2.eaz[2][j]), p2.ebz[2][k]);
                let h = fp_mul_s(p2.w[0], ey0)
                    + fp_mul_s(p2.w[1], ey1)
                    + fp_mul_s(p2.w[2], ey2)
                    + p2.ws;
                let il = h - p;
                let w1 = a0 + ra0j + p2.rzb[0][k] + p2.del1;
                let w2 = a1 + ra1j + p2.rzb[1][k] + p2.del2;
                let qm = fp_mul_s(p2.lam1, fp_mul_s(w1, w1)) / 2
                    + fp_mul_s(p2.lam2, fp_mul_s(w2, w2)) / 2
                    + p2.q0;
                let pay = ((il - p2.d).max(0) - (qm - p2.d).max(0))
                    - ((il - p2.c).max(0) - (qm - p2.c).max(0));
                corr += fp_mul_s(fp_mul_s(p2.ow[i], p2.iw[j * 5 + k]), pay);
            }
        }
    }
    Ok(from_p2(corr))
}

/// tx1: setup + eigendecomp + exp precomp + 7×5×5 Q_model correction.
///
/// Returns `N3Tx1Result` containing the correction and 2D eigenvalues for tx2.
/// Q_model uses only λ₁, λ₂ (λ₃ absorbed into q₀_2d).
pub fn n3_tx1_setup_and_correct(
    w_vol: [i128; 3],
    w_stable: i128,
    sigmas: [i128; 3],
    t_days: u32,
    rho: [[i128; 3]; 3],
    d: i128,
    c: i128,
) -> Result<N3Tx1Result, SolMathError> {
    let t = tx1_phase1(w_vol, w_stable, sigmas, t_days, rho, d, c)?;
    log_cu_marker(); // after phase1
    let correction = tx1_phase2(&t)?;
    log_cu_marker(); // after phase2
    Ok(N3Tx1Result {
        correction,
        lam1: t.lam1,
        lam2: t.lam2,
        del1: t.del1,
        del2: t.del2,
        q0_2d: t.q0_2d,
        d: t.d,
        c: t.c,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Architecture A: no-CV single-tx 7×5×5 with fp_mul_shift
// ═══════════════════════════════════════════════════════════════════════════════

/// Exp tables for the no-CV hot path, heap-allocated.
struct NoCvTables {
    eaz: [[i128; 5]; 3],
    ebz: [[i128; 5]; 3],
    emu: [i128; 3],
    emu_s: i128,
    egamma: [[i128; 7]; 3],
    esigma: [i128; 7],
    w: [i128; 3],
    ws: i128,
    sigma_s: i128,
    d: i128,
    cap_w: i128,
    s2gh7: [i128; 7],
    ow: [i128; 7],
    iw: [i128; 25],
}

/// Phase 1: setup + exp precomputation (no eigendecomp).
#[inline(never)]
fn nocv_setup(
    w_vol: [i128; 3],
    w_stable: i128,
    sigmas: [i128; 3],
    t_days: u32,
    rho: [[i128; 3]; 3],
    d: i128,
    c: i128,
) -> Result<alloc::boxed::Box<NoCvTables>, SolMathError> {
    let t_y = SCALE_I * (t_days as i128) / 365;
    let sigma_mat = build_cov(&sigmas, t_y, &rho)?;
    let mut mu = [0i128; 3];
    for i in 0..3 {
        mu[i] = -fp_mul_i(fp_mul_i(sigmas[i], sigmas[i])?, t_y)? / 2;
    }
    let mu_s = fp_mul_i(w_vol[0], mu[0])? + fp_mul_i(w_vol[1], mu[1])? + fp_mul_i(w_vol[2], mu[2])?;
    let sigma_w = mat3_vec(&sigma_mat, &w_vol)?;
    let var_s = dot3(&w_vol, &sigma_w)?;
    if var_s <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let sigma_s = fp_sqrt(var_s as u128)? as i128;
    let c_vec = [
        fp_div_i(sigma_w[0], var_s)?,
        fp_div_i(sigma_w[1], var_s)?,
        fp_div_i(sigma_w[2], var_s)?,
    ];

    // QR + Cholesky + L_inner
    let q_perp = qr_basis(&w_vol)?;
    let mut v_mat = [[0i128; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            v_mat[i][j] = sigma_mat[i][j] - fp_div_i(fp_mul_i(sigma_w[i], sigma_w[j])?, var_s)?;
        }
    }
    let vq = [mat3_vec(&v_mat, &q_perp[0])?, mat3_vec(&v_mat, &q_perp[1])?];
    let v_proj = [
        [dot3(&q_perp[0], &vq[0])?, dot3(&q_perp[0], &vq[1])?],
        [dot3(&q_perp[1], &vq[0])?, dot3(&q_perp[1], &vq[1])?],
    ];
    if v_proj[0][0] <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let l00 = fp_sqrt(v_proj[0][0] as u128)? as i128;
    let l10 = fp_div_i(v_proj[1][0], l00)?;
    let l11_sq = v_proj[1][1] - fp_mul_i(l10, l10)?;
    let l11 = if l11_sq <= 0 {
        0
    } else {
        fp_sqrt(l11_sq as u128)? as i128
    };
    let mut a_coeff = [0i128; 3];
    let mut b_coeff = [0i128; 3];
    for i in 0..3 {
        a_coeff[i] = fp_mul_i(q_perp[0][i], l00)? + fp_mul_i(q_perp[1][i], l10)?;
        b_coeff[i] = fp_mul_i(q_perp[1][i], l11)?;
    }

    // Exp tables (reciprocal trick, all converted to P2 scale)
    let mut tbl = alloc::boxed::Box::new(NoCvTables {
        eaz: [[0; 5]; 3],
        ebz: [[0; 5]; 3],
        emu: [0; 3],
        emu_s: 0,
        egamma: [[0; 7]; 3],
        esigma: [0; 7],
        w: [to_p2(w_vol[0]), to_p2(w_vol[1]), to_p2(w_vol[2])],
        ws: to_p2(w_stable),
        sigma_s: to_p2(sigma_s),
        d: to_p2(d),
        cap_w: to_p2(c - d),
        s2gh7: [0; 7],
        ow: [0; 7],
        iw: [0; 25],
    });

    // Inner exp tables
    for i in 0..3 {
        tbl.eaz[i][2] = SCALE_P2;
        tbl.eaz[i][3] = to_p2(exp_fixed_i(fp_mul_i(a_coeff[i], SQRT2_GH5[3])?)?);
        tbl.eaz[i][4] = to_p2(exp_fixed_i(fp_mul_i(a_coeff[i], SQRT2_GH5[4])?)?);
        tbl.eaz[i][1] = (SCALE_P2 * SCALE_P2) / tbl.eaz[i][3];
        tbl.eaz[i][0] = (SCALE_P2 * SCALE_P2) / tbl.eaz[i][4];
        tbl.ebz[i][2] = SCALE_P2;
        tbl.ebz[i][3] = to_p2(exp_fixed_i(fp_mul_i(b_coeff[i], SQRT2_GH5[3])?)?);
        tbl.ebz[i][4] = to_p2(exp_fixed_i(fp_mul_i(b_coeff[i], SQRT2_GH5[4])?)?);
        tbl.ebz[i][1] = (SCALE_P2 * SCALE_P2) / tbl.ebz[i][3];
        tbl.ebz[i][0] = (SCALE_P2 * SCALE_P2) / tbl.ebz[i][4];
    }

    // Outer exp tables
    tbl.emu = [
        to_p2(exp_fixed_i(mu[0])?),
        to_p2(exp_fixed_i(mu[1])?),
        to_p2(exp_fixed_i(mu[2])?),
    ];
    tbl.emu_s = to_p2(exp_fixed_i(mu_s)?);
    let gamma = [
        fp_mul_i(c_vec[0], sigma_s)?,
        fp_mul_i(c_vec[1], sigma_s)?,
        fp_mul_i(c_vec[2], sigma_s)?,
    ];
    tbl.esigma[3] = SCALE_P2;
    for m in 4..7 {
        tbl.esigma[m] = to_p2(exp_fixed_i(fp_mul_i(sigma_s, SQRT2_GH7[m])?)?);
    }
    for m in 0..3 {
        tbl.esigma[m] = (SCALE_P2 * SCALE_P2) / tbl.esigma[6 - m];
    }
    for i in 0..3 {
        tbl.egamma[i][3] = SCALE_P2;
        for m in 4..7 {
            tbl.egamma[i][m] = to_p2(exp_fixed_i(fp_mul_i(gamma[i], SQRT2_GH7[m])?)?);
        }
        for m in 0..3 {
            tbl.egamma[i][m] = (SCALE_P2 * SCALE_P2) / tbl.egamma[i][6 - m];
        }
    }

    // Weight + node tables
    for m in 0..7 {
        tbl.s2gh7[m] = to_p2(SQRT2_GH7[m]);
        tbl.ow[m] = to_p2(OUTER_W[m]);
    }
    for j in 0..25 {
        tbl.iw[j] = to_p2(INNER_W_FLAT[j]);
    }

    Ok(tbl)
}

/// Phase 2: 7×5×5 no-CV hot loop with fp_mul_shift.
#[inline(never)]
fn nocv_loop(t: &NoCvTables) -> i128 {
    let mut premium = 0i128;
    for i in 0..7 {
        let p = fp_mul_s(t.emu_s, t.esigma[i]);
        let emc = [
            fp_mul_s(t.emu[0], t.egamma[0][i]),
            fp_mul_s(t.emu[1], t.egamma[1][i]),
            fp_mul_s(t.emu[2], t.egamma[2][i]),
        ];
        for j in 0..5 {
            for k in 0..5 {
                let ey0 = fp_mul_s(fp_mul_s(emc[0], t.eaz[0][j]), t.ebz[0][k]);
                let ey1 = fp_mul_s(fp_mul_s(emc[1], t.eaz[1][j]), t.ebz[1][k]);
                let ey2 = fp_mul_s(fp_mul_s(emc[2], t.eaz[2][j]), t.ebz[2][k]);
                let h =
                    fp_mul_s(t.w[0], ey0) + fp_mul_s(t.w[1], ey1) + fp_mul_s(t.w[2], ey2) + t.ws;
                let il_pay = (h - p - t.d).max(0).min(t.cap_w);
                premium += fp_mul_s(fp_mul_s(t.ow[i], t.iw[j * 5 + k]), il_pay);
            }
        }
    }
    premium
}

/// Architecture A: no-CV single-tx 7×5×5 premium with shift arithmetic.
///
/// Computes `E[min(max(IL − d, 0), c − d)]` directly via GH quadrature.
/// No eigendecomp, no analytical term, no control variate.
/// Returns `(premium, vault_premium)`.
pub fn n3_nocv_shift(
    w_vol: [i128; 3],
    w_stable: i128,
    sigmas: [i128; 3],
    t_days: u32,
    rho: [[i128; 3]; 3],
    d: i128,
    c: i128,
) -> Result<(i128, i128), SolMathError> {
    // Input bounds: weights sum ≤ 1, sigmas in (0, 3], d < c, t_days > 0
    let w_sum = w_vol[0] + w_vol[1] + w_vol[2] + w_stable;
    if w_sum <= 0 || w_sum > SCALE_I + 1000 {
        return Err(SolMathError::DomainError);
    }
    if d >= c || d < 0 || c <= 0 {
        return Err(SolMathError::DomainError);
    }
    if t_days == 0 || t_days > 730 {
        return Err(SolMathError::DomainError);
    }
    for i in 0..3 {
        if sigmas[i] <= 0 || sigmas[i] > 3 * SCALE_I {
            return Err(SolMathError::DomainError);
        }
        if w_vol[i] <= 0 {
            return Err(SolMathError::DomainError);
        }
    }

    log_cu_marker(); // after validation

    let tbl = nocv_setup(w_vol, w_stable, sigmas, t_days, rho, d, c)?;

    log_cu_marker(); // after setup + exp precomp

    let premium_p2 = nocv_loop(&tbl);
    let premium = from_p2(premium_p2);

    log_cu_marker(); // after loop

    let vault_premium = 2 * premium;
    Ok((premium, vault_premium))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Architecture B: no-CV single-tx 9×7×5 with fp_mul_shift (experimental)
// ═══════════════════════════════════════════════════════════════════════════════

/// √2 × GH9_NODES, pre-scaled for 9-point outer quadrature.
const SQRT2_GH9: [i128; 9] = [
    -4_512_745_863_400,
    -3_205_429_002_856,
    -2_076_847_978_678,
    -1_023_255_663_789,
    0,
    1_023_255_663_789,
    2_076_847_978_678,
    3_205_429_002_856,
    4_512_745_863_400,
];

/// Outer weights for 9-point GH: GH9_WEIGHTS[m] / √π, at SCALE_P2 (2^40).
const OUTER_W_9_P2: [i128; 9] = [
    24_569_515,
    3_066_693_314,
    54_883_669_655,
    268_388_042_744,
    446_785_677_318,
    268_388_042_744,
    54_883_669_655,
    3_066_693_314,
    24_569_515,
];

/// Inner weights for 7×5 tensor product: GH7_W[j] × GH5_W[k] / π, at SCALE_P2 (2^40).
/// Flattened 7×5. From hermgauss(7) × hermgauss(5) physicist convention.
const INNER_W_FLAT_7X5_P2: [i128; 35] = [
    // j=0 (GH7 outermost):
    6_786_283,
    133_873_580,
    321_508_258,
    133_873_580,
    6_786_283,
    // j=1:
    380_701_059,
    7_510_122_544,
    18_036_168_234,
    7_510_122_544,
    380_701_059,
    // j=2:
    2_972_161_779,
    58_632_091_182,
    140_809_721_053,
    58_632_091_182,
    2_972_161_779,
    // j=3 (GH7 centre):
    5_658_356_413,
    111_622_883_883,
    268_071_406_392,
    111_622_883_883,
    5_658_356_413,
    // j=4:
    2_972_161_779,
    58_632_091_182,
    140_809_721_053,
    58_632_091_182,
    2_972_161_779,
    // j=5:
    380_701_059,
    7_510_122_544,
    18_036_168_234,
    7_510_122_544,
    380_701_059,
    // j=6 (GH7 outermost):
    6_786_283,
    133_873_580,
    321_508_258,
    133_873_580,
    6_786_283,
];

/// Exp tables for the 9×7×5 no-CV hot path, heap-allocated.
struct NoCv975Tables {
    eaz: [[i128; 7]; 3],    // exp(a[i]·√2·GH7[j])  — inner dim 1
    ebz: [[i128; 5]; 3],    // exp(b[i]·√2·GH5[k])  — inner dim 2
    emu: [i128; 3],         // exp(μ[i])
    emu_s: i128,            // exp(μ_S)
    egamma: [[i128; 9]; 3], // exp(γ[i]·√2·GH9[m])  — outer
    esigma: [i128; 9],      // exp(σ_S·√2·GH9[m])   — outer
    w: [i128; 3],           // weights (P2)
    ws: i128,               // w_stable (P2)
    d: i128,                // deductible (P2)
    cap_w: i128,            // c - d (P2)
}

/// Phase 1: 9×7×5 setup + exp precomputation with Q_perp swap fix.
#[inline(never)]
fn nocv975_setup(
    w_vol: [i128; 3],
    w_stable: i128,
    sigmas: [i128; 3],
    t_days: u32,
    rho: [[i128; 3]; 3],
    d: i128,
    c: i128,
) -> Result<alloc::boxed::Box<NoCv975Tables>, SolMathError> {
    let t_y = SCALE_I * (t_days as i128) / 365;
    let sigma_mat = build_cov(&sigmas, t_y, &rho)?;
    let mut mu = [0i128; 3];
    for i in 0..3 {
        mu[i] = -fp_mul_i(fp_mul_i(sigmas[i], sigmas[i])?, t_y)? / 2;
    }
    let mu_s = fp_mul_i(w_vol[0], mu[0])? + fp_mul_i(w_vol[1], mu[1])? + fp_mul_i(w_vol[2], mu[2])?;
    let sigma_w = mat3_vec(&sigma_mat, &w_vol)?;
    let var_s = dot3(&w_vol, &sigma_w)?;
    if var_s <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let sigma_s = fp_sqrt(var_s as u128)? as i128;
    let c_vec = [
        fp_div_i(sigma_w[0], var_s)?,
        fp_div_i(sigma_w[1], var_s)?,
        fp_div_i(sigma_w[2], var_s)?,
    ];

    // QR basis + sign pin + swap
    let mut q_perp = qr_basis(&w_vol)?;

    // Pin sign convention: first nonzero element of each column positive
    for col in 0..2 {
        for row in 0..3 {
            if q_perp[col][row].abs() > 1000 {
                if q_perp[col][row] < 0 {
                    for r in 0..3 {
                        q_perp[col][r] = -q_perp[col][r];
                    }
                }
                break;
            }
        }
    }

    // Conditional covariance V_proj
    let mut v_mat = [[0i128; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            v_mat[i][j] = sigma_mat[i][j] - fp_div_i(fp_mul_i(sigma_w[i], sigma_w[j])?, var_s)?;
        }
    }
    let vq = [mat3_vec(&v_mat, &q_perp[0])?, mat3_vec(&v_mat, &q_perp[1])?];
    let v_proj_00 = dot3(&q_perp[0], &vq[0])?;
    let v_proj_01 = dot3(&q_perp[0], &vq[1])?;
    let v_proj_11 = dot3(&q_perp[1], &vq[1])?;

    // Swap fix: put higher-variance axis on dim 0 (7 GH nodes)
    let (v00, v01, v11) = if v_proj_11 > v_proj_00 {
        q_perp.swap(0, 1);
        // Re-pin signs after swap
        for col in 0..2 {
            for row in 0..3 {
                if q_perp[col][row].abs() > 1000 {
                    if q_perp[col][row] < 0 {
                        for r in 0..3 {
                            q_perp[col][r] = -q_perp[col][r];
                        }
                    }
                    break;
                }
            }
        }
        (v_proj_11, v_proj_01, v_proj_00)
    } else {
        (v_proj_00, v_proj_01, v_proj_11)
    };

    // 2×2 Cholesky of V_proj
    if v00 <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let l00 = fp_sqrt(v00 as u128)? as i128;
    let l10 = fp_div_i(v01, l00)?;
    let l11_sq = v11 - fp_mul_i(l10, l10)?;
    let l11 = if l11_sq <= 0 {
        0
    } else {
        fp_sqrt(l11_sq as u128)? as i128
    };

    // L_inner columns: a_coeff (col 0) and b_coeff (col 1)
    let mut a_coeff = [0i128; 3];
    let mut b_coeff = [0i128; 3];
    for i in 0..3 {
        a_coeff[i] = fp_mul_i(q_perp[0][i], l00)? + fp_mul_i(q_perp[1][i], l10)?;
        b_coeff[i] = fp_mul_i(q_perp[1][i], l11)?;
    }

    log_cu_marker(); // after QR + Cholesky

    // Allocate tables
    let mut tbl = alloc::boxed::Box::new(NoCv975Tables {
        eaz: [[0; 7]; 3],
        ebz: [[0; 5]; 3],
        emu: [0; 3],
        emu_s: 0,
        egamma: [[0; 9]; 3],
        esigma: [0; 9],
        w: [to_p2(w_vol[0]), to_p2(w_vol[1]), to_p2(w_vol[2])],
        ws: to_p2(w_stable),
        d: to_p2(d),
        cap_w: to_p2(c - d),
    });

    // Inner dim 1 exp tables: eaz[i][j] for GH7 nodes (7 entries)
    for i in 0..3 {
        tbl.eaz[i][3] = SCALE_P2; // zero node
        for m in 4..7 {
            tbl.eaz[i][m] = to_p2(exp_fixed_i(fp_mul_i(a_coeff[i], SQRT2_GH7[m])?)?);
        }
        for m in 0..3 {
            tbl.eaz[i][m] = (SCALE_P2 * SCALE_P2) / tbl.eaz[i][6 - m];
        }
    }

    // Inner dim 2 exp tables: ebz[i][k] for GH5 nodes (5 entries)
    for i in 0..3 {
        tbl.ebz[i][2] = SCALE_P2;
        tbl.ebz[i][3] = to_p2(exp_fixed_i(fp_mul_i(b_coeff[i], SQRT2_GH5[3])?)?);
        tbl.ebz[i][4] = to_p2(exp_fixed_i(fp_mul_i(b_coeff[i], SQRT2_GH5[4])?)?);
        tbl.ebz[i][1] = (SCALE_P2 * SCALE_P2) / tbl.ebz[i][3];
        tbl.ebz[i][0] = (SCALE_P2 * SCALE_P2) / tbl.ebz[i][4];
    }

    // Outer exp tables (separate emu/egamma/esigma — reciprocal trick requires this split)
    tbl.emu = [
        to_p2(exp_fixed_i(mu[0])?),
        to_p2(exp_fixed_i(mu[1])?),
        to_p2(exp_fixed_i(mu[2])?),
    ];
    tbl.emu_s = to_p2(exp_fixed_i(mu_s)?);
    let gamma = [
        fp_mul_i(c_vec[0], sigma_s)?,
        fp_mul_i(c_vec[1], sigma_s)?,
        fp_mul_i(c_vec[2], sigma_s)?,
    ];
    tbl.esigma[4] = SCALE_P2; // zero node
    for m in 5..9 {
        tbl.esigma[m] = to_p2(exp_fixed_i(fp_mul_i(sigma_s, SQRT2_GH9[m])?)?);
    }
    for m in 0..4 {
        tbl.esigma[m] = (SCALE_P2 * SCALE_P2) / tbl.esigma[8 - m];
    }
    for i in 0..3 {
        tbl.egamma[i][4] = SCALE_P2;
        for m in 5..9 {
            tbl.egamma[i][m] = to_p2(exp_fixed_i(fp_mul_i(gamma[i], SQRT2_GH9[m])?)?);
        }
        for m in 0..4 {
            tbl.egamma[i][m] = (SCALE_P2 * SCALE_P2) / tbl.egamma[i][8 - m];
        }
    }

    log_cu_marker(); // after exp precomputation

    Ok(tbl)
}

/// Phase 2: 9×7×5 no-CV hot loop with fp_mul_shift.
#[inline(never)]
fn nocv975_loop(t: &NoCv975Tables) -> i128 {
    let mut premium = 0i128;
    for i in 0..9 {
        let p = fp_mul_s(t.emu_s, t.esigma[i]);
        let emc = [
            fp_mul_s(t.emu[0], t.egamma[0][i]),
            fp_mul_s(t.emu[1], t.egamma[1][i]),
            fp_mul_s(t.emu[2], t.egamma[2][i]),
        ];
        for j in 0..7 {
            for k in 0..5 {
                let ey0 = fp_mul_s(fp_mul_s(emc[0], t.eaz[0][j]), t.ebz[0][k]);
                let ey1 = fp_mul_s(fp_mul_s(emc[1], t.eaz[1][j]), t.ebz[1][k]);
                let ey2 = fp_mul_s(fp_mul_s(emc[2], t.eaz[2][j]), t.ebz[2][k]);
                let h =
                    fp_mul_s(t.w[0], ey0) + fp_mul_s(t.w[1], ey1) + fp_mul_s(t.w[2], ey2) + t.ws;
                let il_pay = (h - p - t.d).max(0).min(t.cap_w);
                premium += fp_mul_s(
                    fp_mul_s(OUTER_W_9_P2[i], INNER_W_FLAT_7X5_P2[j * 5 + k]),
                    il_pay,
                );
            }
        }
    }
    premium
}

/// Architecture B: no-CV single-tx 9×7×5 premium with shift arithmetic.
///
/// Asymmetric grid: 9 outer (GH9), 7 inner-1 (high-var axis), 5 inner-2 (low-var axis).
/// Includes Q_perp swap fix and sign pinning.
/// Returns `(premium, vault_premium)`.
pub fn n3_nocv_975(
    w_vol: [i128; 3],
    w_stable: i128,
    sigmas: [i128; 3],
    t_days: u32,
    rho: [[i128; 3]; 3],
    d: i128,
    c: i128,
) -> Result<(i128, i128), SolMathError> {
    let w_sum = w_vol[0] + w_vol[1] + w_vol[2] + w_stable;
    if w_sum <= 0 || w_sum > SCALE_I + 1000 {
        return Err(SolMathError::DomainError);
    }
    if d >= c || d < 0 || c <= 0 {
        return Err(SolMathError::DomainError);
    }
    if t_days == 0 || t_days > 730 {
        return Err(SolMathError::DomainError);
    }
    for i in 0..3 {
        if sigmas[i] <= 0 || sigmas[i] > 3 * SCALE_I {
            return Err(SolMathError::DomainError);
        }
        if w_vol[i] <= 0 {
            return Err(SolMathError::DomainError);
        }
    }

    log_cu_marker(); // after validation

    let tbl = nocv975_setup(w_vol, w_stable, sigmas, t_days, rho, d, c)?;

    log_cu_marker(); // after setup

    let premium_p2 = nocv975_loop(&tbl);
    let premium = from_p2(premium_p2);

    log_cu_marker(); // after loop

    let vault_premium = 2 * premium;
    Ok((premium, vault_premium))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Architecture C: no-CV single-tx 7×7×7 with fp_mul_shift (experimental)
// ═══════════════════════════════════════════════════════════════════════════════

/// Outer weights for 7-point GH: GH7_W[m] / √π, at SCALE_P2.
const OUTER_W_7_P2: [i128; 7] = [
    602_827_982,
    33_817_815_440,
    264_018_226_975,
    502_633_886_983,
    264_018_226_975,
    33_817_815_440,
    602_827_982,
];

/// Inner weights for 7×7 tensor product: GH7_W[j] × GH7_W[k] / π, at SCALE_P2.
const INNER_W_FLAT_7X7_P2: [i128; 49] = [
    // j=0:
    330_512,
    18_541_255,
    144_752_971,
    275_578_506,
    144_752_971,
    18_541_255,
    330_512,
    // j=1:
    18_541_255,
    1_040_138_742,
    8_120_441_337,
    15_459_572_772,
    8_120_441_337,
    1_040_138_742,
    18_541_255,
    // j=2:
    144_752_971,
    8_120_441_337,
    63_396_895_870,
    120_694_046_617,
    63_396_895_870,
    8_120_441_337,
    144_752_971,
    // j=3:
    275_578_506,
    15_459_572_772,
    120_694_046_617,
    229_775_491_193,
    120_694_046_617,
    15_459_572_772,
    275_578_506,
    // j=4:
    144_752_971,
    8_120_441_337,
    63_396_895_870,
    120_694_046_617,
    63_396_895_870,
    8_120_441_337,
    144_752_971,
    // j=5:
    18_541_255,
    1_040_138_742,
    8_120_441_337,
    15_459_572_772,
    8_120_441_337,
    1_040_138_742,
    18_541_255,
    // j=6:
    330_512,
    18_541_255,
    144_752_971,
    275_578_506,
    144_752_971,
    18_541_255,
    330_512,
];

/// Exp tables for the 7×7×7 no-CV hot path, heap-allocated.
struct NoCv777Tables {
    eaz: [[i128; 7]; 3],    // exp(a[i]·√2·GH7[j])  — inner dim 1
    ebz: [[i128; 7]; 3],    // exp(b[i]·√2·GH7[k])  — inner dim 2
    emu: [i128; 3],         // exp(μ[i])
    emu_s: i128,            // exp(μ_S)
    egamma: [[i128; 7]; 3], // exp(γ[i]·√2·GH7[m])  — outer
    esigma: [i128; 7],      // exp(σ_S·√2·GH7[m])   — outer
    w: [i128; 3],           // weights (P2)
    ws: i128,               // w_stable (P2)
    d: i128,                // deductible (P2)
    cap_w: i128,            // c - d (P2)
}

/// Phase 1: 7×7×7 setup + exp precomputation.
#[inline(never)]
fn nocv777_setup(
    w_vol: [i128; 3],
    w_stable: i128,
    sigmas: [i128; 3],
    t_days: u32,
    rho: [[i128; 3]; 3],
    d: i128,
    c: i128,
) -> Result<alloc::boxed::Box<NoCv777Tables>, SolMathError> {
    let t_y = SCALE_I * (t_days as i128) / 365;
    let sigma_mat = build_cov(&sigmas, t_y, &rho)?;
    let mut mu = [0i128; 3];
    for i in 0..3 {
        mu[i] = -fp_mul_i(fp_mul_i(sigmas[i], sigmas[i])?, t_y)? / 2;
    }
    let mu_s = fp_mul_i(w_vol[0], mu[0])? + fp_mul_i(w_vol[1], mu[1])? + fp_mul_i(w_vol[2], mu[2])?;
    let sigma_w = mat3_vec(&sigma_mat, &w_vol)?;
    let var_s = dot3(&w_vol, &sigma_w)?;
    if var_s <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let sigma_s = fp_sqrt(var_s as u128)? as i128;
    let c_vec = [
        fp_div_i(sigma_w[0], var_s)?,
        fp_div_i(sigma_w[1], var_s)?,
        fp_div_i(sigma_w[2], var_s)?,
    ];

    // QR + Cholesky (no swap needed — symmetric grid)
    let q_perp = qr_basis(&w_vol)?;
    let mut v_mat = [[0i128; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            v_mat[i][j] = sigma_mat[i][j] - fp_div_i(fp_mul_i(sigma_w[i], sigma_w[j])?, var_s)?;
        }
    }
    let vq = [mat3_vec(&v_mat, &q_perp[0])?, mat3_vec(&v_mat, &q_perp[1])?];
    let v_proj = [
        [dot3(&q_perp[0], &vq[0])?, dot3(&q_perp[0], &vq[1])?],
        [dot3(&q_perp[1], &vq[0])?, dot3(&q_perp[1], &vq[1])?],
    ];
    if v_proj[0][0] <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let l00 = fp_sqrt(v_proj[0][0] as u128)? as i128;
    let l10 = fp_div_i(v_proj[1][0], l00)?;
    let l11_sq = v_proj[1][1] - fp_mul_i(l10, l10)?;
    let l11 = if l11_sq <= 0 {
        0
    } else {
        fp_sqrt(l11_sq as u128)? as i128
    };
    let mut a_coeff = [0i128; 3];
    let mut b_coeff = [0i128; 3];
    for i in 0..3 {
        a_coeff[i] = fp_mul_i(q_perp[0][i], l00)? + fp_mul_i(q_perp[1][i], l10)?;
        b_coeff[i] = fp_mul_i(q_perp[1][i], l11)?;
    }

    log_cu_marker();

    // Allocate tables
    let mut tbl = alloc::boxed::Box::new(NoCv777Tables {
        eaz: [[0; 7]; 3],
        ebz: [[0; 7]; 3],
        emu: [0; 3],
        emu_s: 0,
        egamma: [[0; 7]; 3],
        esigma: [0; 7],
        w: [to_p2(w_vol[0]), to_p2(w_vol[1]), to_p2(w_vol[2])],
        ws: to_p2(w_stable),
        d: to_p2(d),
        cap_w: to_p2(c - d),
    });

    // Inner dim 1: eaz for GH7
    for i in 0..3 {
        tbl.eaz[i][3] = SCALE_P2;
        for m in 4..7 {
            tbl.eaz[i][m] = to_p2(exp_fixed_i(fp_mul_i(a_coeff[i], SQRT2_GH7[m])?)?);
        }
        for m in 0..3 {
            tbl.eaz[i][m] = (SCALE_P2 * SCALE_P2) / tbl.eaz[i][6 - m];
        }
    }

    // Inner dim 2: ebz for GH7 (same structure, b_coeff instead of a_coeff)
    for i in 0..3 {
        tbl.ebz[i][3] = SCALE_P2;
        for m in 4..7 {
            tbl.ebz[i][m] = to_p2(exp_fixed_i(fp_mul_i(b_coeff[i], SQRT2_GH7[m])?)?);
        }
        for m in 0..3 {
            tbl.ebz[i][m] = (SCALE_P2 * SCALE_P2) / tbl.ebz[i][6 - m];
        }
    }

    // Outer: emu, emu_s, egamma, esigma (all GH7)
    tbl.emu = [
        to_p2(exp_fixed_i(mu[0])?),
        to_p2(exp_fixed_i(mu[1])?),
        to_p2(exp_fixed_i(mu[2])?),
    ];
    tbl.emu_s = to_p2(exp_fixed_i(mu_s)?);
    let gamma = [
        fp_mul_i(c_vec[0], sigma_s)?,
        fp_mul_i(c_vec[1], sigma_s)?,
        fp_mul_i(c_vec[2], sigma_s)?,
    ];
    tbl.esigma[3] = SCALE_P2;
    for m in 4..7 {
        tbl.esigma[m] = to_p2(exp_fixed_i(fp_mul_i(sigma_s, SQRT2_GH7[m])?)?);
    }
    for m in 0..3 {
        tbl.esigma[m] = (SCALE_P2 * SCALE_P2) / tbl.esigma[6 - m];
    }
    for i in 0..3 {
        tbl.egamma[i][3] = SCALE_P2;
        for m in 4..7 {
            tbl.egamma[i][m] = to_p2(exp_fixed_i(fp_mul_i(gamma[i], SQRT2_GH7[m])?)?);
        }
        for m in 0..3 {
            tbl.egamma[i][m] = (SCALE_P2 * SCALE_P2) / tbl.egamma[i][6 - m];
        }
    }

    log_cu_marker();

    Ok(tbl)
}

/// Phase 2: 7×7×7 no-CV hot loop with fp_mul_shift.
#[inline(never)]
fn nocv777_loop(t: &NoCv777Tables) -> i128 {
    let mut premium = 0i128;
    for i in 0..7 {
        let p = fp_mul_s(t.emu_s, t.esigma[i]);
        let emc = [
            fp_mul_s(t.emu[0], t.egamma[0][i]),
            fp_mul_s(t.emu[1], t.egamma[1][i]),
            fp_mul_s(t.emu[2], t.egamma[2][i]),
        ];
        for j in 0..7 {
            for k in 0..7 {
                let ey0 = fp_mul_s(fp_mul_s(emc[0], t.eaz[0][j]), t.ebz[0][k]);
                let ey1 = fp_mul_s(fp_mul_s(emc[1], t.eaz[1][j]), t.ebz[1][k]);
                let ey2 = fp_mul_s(fp_mul_s(emc[2], t.eaz[2][j]), t.ebz[2][k]);
                let h =
                    fp_mul_s(t.w[0], ey0) + fp_mul_s(t.w[1], ey1) + fp_mul_s(t.w[2], ey2) + t.ws;
                let il_pay = (h - p - t.d).max(0).min(t.cap_w);
                premium += fp_mul_s(
                    fp_mul_s(OUTER_W_7_P2[i], INNER_W_FLAT_7X7_P2[j * 7 + k]),
                    il_pay,
                );
            }
        }
    }
    premium
}

/// Architecture C: no-CV single-tx 7×7×7 premium with shift arithmetic.
///
/// Symmetric grid: all three GH axes use 7 nodes. 343 total nodes.
/// No rotation or swap needed — orientation-invariant.
/// Returns `(premium, vault_premium)`.
pub fn n3_nocv_777(
    w_vol: [i128; 3],
    w_stable: i128,
    sigmas: [i128; 3],
    t_days: u32,
    rho: [[i128; 3]; 3],
    d: i128,
    c: i128,
) -> Result<(i128, i128), SolMathError> {
    let w_sum = w_vol[0] + w_vol[1] + w_vol[2] + w_stable;
    if w_sum <= 0 || w_sum > SCALE_I + 1000 {
        return Err(SolMathError::DomainError);
    }
    if d >= c || d < 0 || c <= 0 {
        return Err(SolMathError::DomainError);
    }
    if t_days == 0 || t_days > 730 {
        return Err(SolMathError::DomainError);
    }
    for i in 0..3 {
        if sigmas[i] <= 0 || sigmas[i] > 3 * SCALE_I {
            return Err(SolMathError::DomainError);
        }
        if w_vol[i] <= 0 {
            return Err(SolMathError::DomainError);
        }
    }

    log_cu_marker();

    let tbl = nocv777_setup(w_vol, w_stable, sigmas, t_days, rho, d, c)?;

    log_cu_marker();

    let premium_p2 = nocv777_loop(&tbl);
    let premium = from_p2(premium_p2);

    log_cu_marker();

    let vault_premium = 2 * premium;
    Ok((premium, vault_premium))
}

/// tx2: 2D analytical spread (λ₃ absorbed into q₀_2d).
///
/// Runs spread_split with λ₁, λ₂ only. No GH3 nesting needed.
pub fn n3_tx2_analytical(tx1: &N3Tx1Result) -> Result<(i128, i128), SolMathError> {
    let analytical = spread_split(
        tx1.lam1, tx1.lam2, tx1.del1, tx1.del2, tx1.q0_2d, tx1.d, tx1.c,
    )?;
    let premium = analytical + tx1.correction;
    let vault_premium = 2 * premium;
    Ok((premium, vault_premium))
}

/// Compute the 7×5×5 tensor GH correction (tx2 equivalent).
///
/// Returns the correction amount at SCALE. Final premium = setup.analytical + correction.
pub fn n3_cv_correction(setup: &N3CvSetup) -> Result<i128, SolMathError> {
    let d = setup.d;
    let c = setup.c;
    let w = &setup.w_vol;
    let w_stable = setup.w_stable;

    let mut correction = 0i128;

    for i in 0..7 {
        // Outer node: s = μ_S + σ_S × √2 × GH7_NODES[i]
        let s = setup.mu_s + fp_mul_i(setup.sigma_s, SQRT2_GH7[i])?;
        let exp_s = exp_fixed_i(s)?;
        let p = exp_s;

        let ds = s - setup.mu_s;
        let mut mu_cond = [0i128; 3];
        let mut exp_mu = [0i128; 3];
        for a in 0..3 {
            mu_cond[a] = setup.mu[a] + fp_mul_i(setup.c_vec[a], ds)?;
            exp_mu[a] = exp_fixed_i(mu_cond[a])?;
        }

        let s_sq = fp_mul_i(s, s)?;

        // Outer weight: GH7_WEIGHTS[i] / √π
        let outer_w = fp_mul_i(GH7_WEIGHTS[i], INV_SQRT_PI)?;

        for j in 0..5 {
            for k in 0..5 {
                // exp(Y[a]) = exp_mu[a] × exp_az[a][j] × exp_bz[a][k]
                let mut h = w_stable;
                for a in 0..3 {
                    let exp_y_a =
                        fp_mul_i(fp_mul_i(exp_mu[a], setup.exp_az[a][j])?, setup.exp_bz[a][k])?;
                    h += fp_mul_i(w[a], exp_y_a)?;
                }

                let il = h - p;

                // Q_full = ½(Σ wᵢYᵢ² − s²)
                let mut w_y_sq_sum = 0i128;
                for a in 0..3 {
                    let y_a = mu_cond[a]
                        + fp_mul_i(setup.a_coeff[a], SQRT2_GH5[j])?
                        + fp_mul_i(setup.b_coeff[a], SQRT2_GH5[k])?;
                    w_y_sq_sum += fp_mul_i(w[a], fp_mul_i(y_a, y_a)?)?;
                }
                let q_val = (w_y_sq_sum - s_sq) / 2;

                // Correction = (max(IL−d,0) − max(Q−d,0)) − (max(IL−c,0) − max(Q−c,0))
                let il_d = (il - d).max(0);
                let q_d = (q_val - d).max(0);
                let il_c = (il - c).max(0);
                let q_c = (q_val - c).max(0);
                let node_corr = (il_d - q_d) - (il_c - q_c);

                // Inner weight: GH5_WEIGHTS[j] × GH5_WEIGHTS[k] / π
                let inner_w = fp_mul_i(fp_mul_i(GH5_WEIGHTS[j], GH5_WEIGHTS[k])?, INV_PI)?;

                let total_w = fp_mul_i(outer_w, inner_w)?;
                correction += fp_mul_i(total_w, node_corr)?;
            }
        }
    }

    Ok(correction)
}

/// Compute the full N=3 CV premium in a single call.
///
/// Returns `(premium, vault_premium)` where `vault_premium = premium × 2`.
pub fn n3_cv_premium(
    w_vol: [i128; 3],
    w_stable: i128,
    sigmas: [i128; 3],
    t_days: u32,
    rho: [[i128; 3]; 3],
    d: i128,
    c: i128,
) -> Result<(i128, i128), SolMathError> {
    let setup = n3_cv_setup(w_vol, w_stable, sigmas, t_days, rho, d, c)?;
    let correction = n3_cv_correction(&setup)?;
    let premium = setup.analytical + correction;
    let vault_premium = 2 * premium;
    Ok((premium, vault_premium))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tier 2 — Precise: Q_full correction (tx1) + GL5×GH3 analytical (tx2)
// ═══════════════════════════════════════════════════════════════════════════════

/// Result from Tier 2 tx1.
pub struct N3Tier2Tx1Result {
    pub correction: i128,
    pub lam1: i128,
    pub lam2: i128,
    pub lam3: i128,
    pub del1: i128,
    pub del2: i128,
    pub del3: i128,
    pub q0: i128,
    pub d: i128,
    pub c: i128,
}

/// Tables for Tier 2 Q_full correction loop, heap-allocated.
struct Tier2Tables {
    // H/IL computation (same as NoCvTables)
    eaz: [[i128; 5]; 3],
    ebz: [[i128; 5]; 3],
    emu: [i128; 3],
    emu_s: i128,
    egamma: [[i128; 7]; 3],
    esigma: [i128; 7],
    w: [i128; 3],
    ws: i128,
    sigma_s: i128,
    d: i128,
    c: i128,
    s2gh7: [i128; 7],
    ow: [i128; 7],
    iw: [i128; 25],
    // Q_full: Y_i computation
    dy: [[[i128; 5]; 5]; 3], // dY[i][j][k] = a_i √2 z_j + b_i √2 z_k in P2
    mu_p2: [i128; 3],
    cvec_p2: [i128; 3],
    mu_s_p2: i128,
}

/// Tier 2 phase 1: setup + eigendecomp + exp precomp + dY precomp.
#[inline(never)]
fn tier2_setup(
    w_vol: [i128; 3],
    w_stable: i128,
    sigmas: [i128; 3],
    t_days: u32,
    rho: [[i128; 3]; 3],
    d: i128,
    c: i128,
) -> Result<(alloc::boxed::Box<Tier2Tables>, EigenFull), SolMathError> {
    let t_y = SCALE_I * (t_days as i128) / 365;
    let sigma_mat = build_cov(&sigmas, t_y, &rho)?;
    let mut mu = [0i128; 3];
    for i in 0..3 {
        mu[i] = -fp_mul_i(fp_mul_i(sigmas[i], sigmas[i])?, t_y)? / 2;
    }
    let mu_s = fp_mul_i(w_vol[0], mu[0])? + fp_mul_i(w_vol[1], mu[1])? + fp_mul_i(w_vol[2], mu[2])?;
    let sigma_w = mat3_vec(&sigma_mat, &w_vol)?;
    let var_s = dot3(&w_vol, &sigma_w)?;
    if var_s <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let sigma_s = fp_sqrt(var_s as u128)? as i128;
    let c_vec = [
        fp_div_i(sigma_w[0], var_s)?,
        fp_div_i(sigma_w[1], var_s)?,
        fp_div_i(sigma_w[2], var_s)?,
    ];

    let q_perp = qr_basis(&w_vol)?;
    let mut v_mat = [[0i128; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            v_mat[i][j] = sigma_mat[i][j] - fp_div_i(fp_mul_i(sigma_w[i], sigma_w[j])?, var_s)?;
        }
    }
    let vq = [mat3_vec(&v_mat, &q_perp[0])?, mat3_vec(&v_mat, &q_perp[1])?];
    let v_proj = [
        [dot3(&q_perp[0], &vq[0])?, dot3(&q_perp[0], &vq[1])?],
        [dot3(&q_perp[1], &vq[0])?, dot3(&q_perp[1], &vq[1])?],
    ];
    if v_proj[0][0] <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let l00 = fp_sqrt(v_proj[0][0] as u128)? as i128;
    let l10 = fp_div_i(v_proj[1][0], l00)?;
    let l11_sq = v_proj[1][1] - fp_mul_i(l10, l10)?;
    let l11 = if l11_sq <= 0 {
        0
    } else {
        fp_sqrt(l11_sq as u128)? as i128
    };
    let mut a_coeff = [0i128; 3];
    let mut b_coeff = [0i128; 3];
    for i in 0..3 {
        a_coeff[i] = fp_mul_i(q_perp[0][i], l00)? + fp_mul_i(q_perp[1][i], l10)?;
        b_coeff[i] = fp_mul_i(q_perp[1][i], l11)?;
    }

    // Exp tables (reciprocal trick, converted to P2)
    let mut tbl = alloc::boxed::Box::new(Tier2Tables {
        eaz: [[0; 5]; 3],
        ebz: [[0; 5]; 3],
        emu: [0; 3],
        emu_s: 0,
        egamma: [[0; 7]; 3],
        esigma: [0; 7],
        w: [to_p2(w_vol[0]), to_p2(w_vol[1]), to_p2(w_vol[2])],
        ws: to_p2(w_stable),
        sigma_s: to_p2(sigma_s),
        d: to_p2(d),
        c: to_p2(c),
        s2gh7: [0; 7],
        ow: [0; 7],
        iw: [0; 25],
        dy: [[[0; 5]; 5]; 3],
        mu_p2: [to_p2(mu[0]), to_p2(mu[1]), to_p2(mu[2])],
        cvec_p2: [to_p2(c_vec[0]), to_p2(c_vec[1]), to_p2(c_vec[2])],
        mu_s_p2: to_p2(mu_s),
    });
    for i in 0..3 {
        tbl.eaz[i][2] = SCALE_P2;
        tbl.eaz[i][3] = to_p2(exp_fixed_i(fp_mul_i(a_coeff[i], SQRT2_GH5[3])?)?);
        tbl.eaz[i][4] = to_p2(exp_fixed_i(fp_mul_i(a_coeff[i], SQRT2_GH5[4])?)?);
        tbl.eaz[i][1] = (SCALE_P2 * SCALE_P2) / tbl.eaz[i][3];
        tbl.eaz[i][0] = (SCALE_P2 * SCALE_P2) / tbl.eaz[i][4];
        tbl.ebz[i][2] = SCALE_P2;
        tbl.ebz[i][3] = to_p2(exp_fixed_i(fp_mul_i(b_coeff[i], SQRT2_GH5[3])?)?);
        tbl.ebz[i][4] = to_p2(exp_fixed_i(fp_mul_i(b_coeff[i], SQRT2_GH5[4])?)?);
        tbl.ebz[i][1] = (SCALE_P2 * SCALE_P2) / tbl.ebz[i][3];
        tbl.ebz[i][0] = (SCALE_P2 * SCALE_P2) / tbl.ebz[i][4];
    }
    tbl.emu = [
        to_p2(exp_fixed_i(mu[0])?),
        to_p2(exp_fixed_i(mu[1])?),
        to_p2(exp_fixed_i(mu[2])?),
    ];
    tbl.emu_s = to_p2(exp_fixed_i(mu_s)?);
    let gamma = [
        fp_mul_i(c_vec[0], sigma_s)?,
        fp_mul_i(c_vec[1], sigma_s)?,
        fp_mul_i(c_vec[2], sigma_s)?,
    ];
    tbl.esigma[3] = SCALE_P2;
    for m in 4..7 {
        tbl.esigma[m] = to_p2(exp_fixed_i(fp_mul_i(sigma_s, SQRT2_GH7[m])?)?);
    }
    for m in 0..3 {
        tbl.esigma[m] = (SCALE_P2 * SCALE_P2) / tbl.esigma[6 - m];
    }
    for i in 0..3 {
        tbl.egamma[i][3] = SCALE_P2;
        for m in 4..7 {
            tbl.egamma[i][m] = to_p2(exp_fixed_i(fp_mul_i(gamma[i], SQRT2_GH7[m])?)?);
        }
        for m in 0..3 {
            tbl.egamma[i][m] = (SCALE_P2 * SCALE_P2) / tbl.egamma[i][6 - m];
        }
    }
    for m in 0..7 {
        tbl.s2gh7[m] = to_p2(SQRT2_GH7[m]);
        tbl.ow[m] = to_p2(OUTER_W[m]);
    }
    for j in 0..25 {
        tbl.iw[j] = to_p2(INNER_W_FLAT[j]);
    }

    // dY precomp in P2: dy[i][j][k] = a_i √2 GH5_j + b_i √2 GH5_k
    for i in 0..3 {
        for j in 0..5 {
            let a_term = to_p2(fp_mul_i(a_coeff[i], SQRT2_GH5[j])?);
            for k in 0..5 {
                tbl.dy[i][j][k] = a_term + to_p2(fp_mul_i(b_coeff[i], SQRT2_GH5[k])?);
            }
        }
    }

    // Eigendecomp (standard SCALE, for tx2)
    let ef = analytical_eigen_full(&sigma_mat, &w_vol, &mu)?;

    Ok((tbl, ef))
}

/// Tier 2 phase 2: 7×5×5 Q_full correction with rounded shift arithmetic.
#[inline(never)]
fn tier2_correction_loop(t: &Tier2Tables) -> i128 {
    let mut corr = 0i128;
    for i in 0..7 {
        let p = fp_mul_s(t.emu_s, t.esigma[i]);
        let emc = [
            fp_mul_s(t.emu[0], t.egamma[0][i]),
            fp_mul_s(t.emu[1], t.egamma[1][i]),
            fp_mul_s(t.emu[2], t.egamma[2][i]),
        ];
        let ds = fp_mul_s(t.sigma_s, t.s2gh7[i]);
        let s_lin = t.mu_s_p2 + ds;
        let s_sq = fp_mul_s(s_lin, s_lin);
        let mc = [
            t.mu_p2[0] + fp_mul_s(t.cvec_p2[0], ds),
            t.mu_p2[1] + fp_mul_s(t.cvec_p2[1], ds),
            t.mu_p2[2] + fp_mul_s(t.cvec_p2[2], ds),
        ];
        for j in 0..5 {
            for k in 0..5 {
                let ey0 = fp_mul_s(fp_mul_s(emc[0], t.eaz[0][j]), t.ebz[0][k]);
                let ey1 = fp_mul_s(fp_mul_s(emc[1], t.eaz[1][j]), t.ebz[1][k]);
                let ey2 = fp_mul_s(fp_mul_s(emc[2], t.eaz[2][j]), t.ebz[2][k]);
                let h =
                    fp_mul_s(t.w[0], ey0) + fp_mul_s(t.w[1], ey1) + fp_mul_s(t.w[2], ey2) + t.ws;
                let il = h - p;
                let y0 = mc[0] + t.dy[0][j][k];
                let y1 = mc[1] + t.dy[1][j][k];
                let y2 = mc[2] + t.dy[2][j][k];
                let wyq = fp_mul_s(t.w[0], fp_mul_s(y0, y0))
                    + fp_mul_s(t.w[1], fp_mul_s(y1, y1))
                    + fp_mul_s(t.w[2], fp_mul_s(y2, y2));
                let qv = (wyq - s_sq) / 2;
                let pay = ((il - t.d).max(0) - (qv - t.d).max(0))
                    - ((il - t.c).max(0) - (qv - t.c).max(0));
                corr += fp_mul_s(fp_mul_s(t.ow[i], t.iw[j * 5 + k]), pay);
            }
        }
    }
    corr
}

/// Tier 2 tx1: setup + eigendecomp + exp precomp + 7×5×5 Q_full correction.
pub fn n3_tier2_tx1(
    w_vol: [i128; 3],
    w_stable: i128,
    sigmas: [i128; 3],
    t_days: u32,
    rho: [[i128; 3]; 3],
    d: i128,
    c: i128,
) -> Result<N3Tier2Tx1Result, SolMathError> {
    let (tbl, ef) = tier2_setup(w_vol, w_stable, sigmas, t_days, rho, d, c)?;
    log_cu_marker(); // after setup + eigen + exp
    let correction = from_p2(tier2_correction_loop(&tbl));
    log_cu_marker(); // after correction
    Ok(N3Tier2Tx1Result {
        correction,
        lam1: ef.lam1,
        lam2: ef.lam2,
        lam3: ef.lam3,
        del1: ef.del1,
        del2: ef.del2,
        del3: ef.del3,
        q0: ef.q0,
        d,
        c,
    })
}

/// Tier 2 tx2: GL5×GH3 analytical spread. Premium = analytical + correction.
pub fn n3_tier2_tx2(tx1: &N3Tier2Tx1Result) -> Result<(i128, i128), SolMathError> {
    let analytical = spread_split_3d_gl5(
        tx1.lam1, tx1.lam2, tx1.lam3, tx1.del1, tx1.del2, tx1.del3, tx1.q0, tx1.d, tx1.c,
    )?;
    let premium = analytical + tx1.correction;
    Ok((premium, 2 * premium))
}

/// Direct 9×5×5 tensor GH premium (no control variate).
///
/// Accumulates `min(max(IL − d, 0), c − d)` at each node.
/// Avoids CV bias from eigendecomposition mismatch.
pub fn n3_noCV_premium(setup: &N3CvSetup) -> Result<i128, SolMathError> {
    let d = setup.d;
    let c = setup.c;
    let cap_w = c - d;
    let w = &setup.w_vol;
    let w_stable = setup.w_stable;

    let mut premium = 0i128;

    for i in 0..7 {
        let s = setup.mu_s + fp_mul_i(setup.sigma_s, SQRT2_GH7[i])?;
        let exp_s = exp_fixed_i(s)?;
        let p = exp_s;

        let ds = s - setup.mu_s;
        let mut exp_mu = [0i128; 3];
        for a in 0..3 {
            let mu_cond_a = setup.mu[a] + fp_mul_i(setup.c_vec[a], ds)?;
            exp_mu[a] = exp_fixed_i(mu_cond_a)?;
        }

        let outer_w = fp_mul_i(GH7_WEIGHTS[i], INV_SQRT_PI)?;

        for j in 0..5 {
            for k in 0..5 {
                let mut h = w_stable;
                for a in 0..3 {
                    let exp_y_a =
                        fp_mul_i(fp_mul_i(exp_mu[a], setup.exp_az[a][j])?, setup.exp_bz[a][k])?;
                    h += fp_mul_i(w[a], exp_y_a)?;
                }

                let il = h - p;
                let il_pay = (il - d).max(0).min(cap_w);

                let inner_w = fp_mul_i(fp_mul_i(GH5_WEIGHTS[j], GH5_WEIGHTS[k])?, INV_PI)?;
                let total_w = fp_mul_i(outer_w, inner_w)?;
                premium += fp_mul_i(total_w, il_pay)?;
            }
        }
    }

    Ok(premium)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Single-transaction N=3 premium engine (~971K CU)
// ═══════════════════════════════════════════════════════════════════════════════

/// Pre-divided outer weights: GH7_WEIGHTS / √π at SCALE.
const OUTER_W: [i128; 7] = [
    548_268_856,
    30_757_123_968,
    240_123_178_605,
    457_142_857_143,
    240_123_178_605,
    30_757_123_968,
    548_268_856,
];

/// Pre-divided inner weights: GH5_W[j] × GH5_W[k] / π, flattened 5×5.
const INNER_W_FLAT: [i128; 25] = [
    126_729_310,
    2_500_000_000,
    6_003_952_708,
    2_500_000_000,
    126_729_310,
    2_500_000_000,
    49_317_715_135,
    118_440_491_736,
    49_317_715_135,
    2_500_000_000,
    6_003_952_708,
    118_440_491_736,
    284_444_444_444,
    118_440_491_736,
    6_003_952_708,
    2_500_000_000,
    49_317_715_135,
    118_440_491_736,
    49_317_715_135,
    2_500_000_000,
    126_729_310,
    2_500_000_000,
    6_003_952_708,
    2_500_000_000,
    126_729_310,
];

/// SCALE² = 1e24, used for reciprocals.
const SCALE_SQ: i128 = 1_000_000_000_000_000_000_000_000;

// ─── PoolConstants ───────────────────────────────────────────────────────────

/// Weight-only constants stored in the pool PDA at creation time.
///
/// These depend only on weights and never change.
#[derive(Clone, Debug)]
pub struct PoolConstants {
    /// Q_perp transposed: 2 rows × 3 cols — orthonormal basis ⊥ to w.
    pub q_perp: [[i128; 3]; 2],
    /// M_proj = Q_perpᵀ M Q_perp, symmetric 2×2 stored as [m00, m01, m11].
    pub m_proj: [i128; 3],
    /// Volatile asset weights at SCALE.
    pub weights: [i128; 3],
    /// Stablecoin weight at SCALE.
    pub w_stable: i128,
}

/// Compute the weight-only pool constants for the N=3 premium engine.
///
/// Call this once when the pool is created. The result is stored in the pool PDA.
///
/// `w_vol`: weights for 3 volatile assets at SCALE (must sum with w_stable to SCALE).
/// `w_stable`: stablecoin weight at SCALE.
pub fn compute_pool_constants(
    w_vol: [i128; 3],
    w_stable: i128,
) -> Result<PoolConstants, SolMathError> {
    // 1. QR basis: Q_perp = two columns orthogonal to w
    let q_cols = qr_basis(&w_vol)?;
    // Store transposed: q_perp[row][col] = q_cols[row][col]
    let q_perp = q_cols;

    // 2. M = diag(w) − wwᵀ
    //    M_proj = Q_perpᵀ M Q_perp (2×2 symmetric, 3 unique values)
    //
    //    M_proj[a][b] = Σᵢ wᵢ · q_perp[a][i] · q_perp[b][i]
    //                 − (Σᵢ wᵢ q_perp[a][i]) (Σⱼ wⱼ q_perp[b][j])
    //
    //    But since Q_perp ⊥ w: Σᵢ wᵢ q_perp[a][i] = 0.
    //    So M_proj[a][b] = Σᵢ wᵢ · q_perp[a][i] · q_perp[b][i].
    let mut m00 = 0i128;
    let mut m01 = 0i128;
    let mut m11 = 0i128;
    for i in 0..3 {
        m00 += fp_mul_i(w_vol[i], fp_mul_i(q_perp[0][i], q_perp[0][i])?)?;
        m01 += fp_mul_i(w_vol[i], fp_mul_i(q_perp[0][i], q_perp[1][i])?)?;
        m11 += fp_mul_i(w_vol[i], fp_mul_i(q_perp[1][i], q_perp[1][i])?)?;
    }

    Ok(PoolConstants {
        q_perp,
        m_proj: [m00, m01, m11],
        weights: w_vol,
        w_stable,
    })
}

// ─── Single-transaction compute_n3_premium ───────────────────────────────────

/// Compute the N=3 IL insurance premium in a single Solana transaction (~971K CU).
///
/// Takes precomputed pool constants (from PDA) and market data.
/// Returns `(premium, vault_premium)` where `vault_premium = 2 × premium`.
///
/// # Arguments
/// * `pool` — Weight-only constants from `compute_pool_constants`.
/// * `sigmas` — Annual volatilities at SCALE.
/// * `t_days` — Tenor in days.
/// * `rho` — 3×3 correlation matrix at SCALE.
/// * `d` — Deductible at SCALE.
/// * `c` — Cap at SCALE.
pub fn compute_n3_premium(
    pool: &PoolConstants,
    sigmas: [i128; 3],
    t_days: u32,
    rho: [[i128; 3]; 3],
    d: i128,
    c: i128,
) -> Result<(i128, i128), SolMathError> {
    let w = &pool.weights;
    let t_y = SCALE_I * (t_days as i128) / 365;

    // ═══ Step 1: Setup (~30K CU) ═══════════════════════════════════════════

    // Covariance Σᵢⱼ = ρᵢⱼ σᵢ σⱼ T
    let sigma_mat = build_cov(&sigmas, t_y, &rho)?;

    // Drift μᵢ = −σᵢ²T/2
    let mut mu = [0i128; 3];
    for i in 0..3 {
        mu[i] = -fp_mul_i(fp_mul_i(sigmas[i], sigmas[i])?, t_y)? / 2;
    }

    // μ_S = wᵀμ
    let mu_s = fp_mul_i(w[0], mu[0])? + fp_mul_i(w[1], mu[1])? + fp_mul_i(w[2], mu[2])?;

    // var_S = wᵀΣw, σ_S = √var_S
    let sigma_w = mat3_vec(&sigma_mat, w)?;
    let var_s = dot3(w, &sigma_w)?;
    if var_s <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let sigma_s = fp_sqrt(var_s as u128)? as i128;

    // c_vec = Σw / var_S
    let c_vec = [
        fp_div_i(sigma_w[0], var_s)?,
        fp_div_i(sigma_w[1], var_s)?,
        fp_div_i(sigma_w[2], var_s)?,
    ];

    // V_proj = Q_perpᵀ Σ Q_perp (2×2, the wwᵀ term vanishes)
    let sq0 = mat3_vec(&sigma_mat, &pool.q_perp[0])?;
    let sq1 = mat3_vec(&sigma_mat, &pool.q_perp[1])?;
    let v_proj_00 = dot3(&pool.q_perp[0], &sq0)?;
    let v_proj_01 = dot3(&pool.q_perp[0], &sq1)?;
    let v_proj_11 = dot3(&pool.q_perp[1], &sq1)?;

    // 2×2 Cholesky: L_proj
    if v_proj_00 <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let l00 = fp_sqrt(v_proj_00 as u128)? as i128;
    let l10 = fp_div_i(v_proj_01, l00)?;
    let l11_sq = v_proj_11 - fp_mul_i(l10, l10)?;
    let l11 = if l11_sq <= 0 {
        0
    } else {
        fp_sqrt(l11_sq as u128)? as i128
    };

    // L_inner = Q_perp @ L_proj (3×2) → a[i], b[i]
    let mut a_coeff = [0i128; 3];
    let mut b_coeff = [0i128; 3];
    for i in 0..3 {
        a_coeff[i] = fp_mul_i(pool.q_perp[0][i], l00)? + fp_mul_i(pool.q_perp[1][i], l10)?;
        b_coeff[i] = fp_mul_i(pool.q_perp[1][i], l11)?;
    }

    // ═══ Step 2: Exp precomputation (~220K CU) ═════════════════════════════

    // Table 1: exp_az[i][j] = exp(a[i] × √2 × GH5_NODES[j])
    // Symmetry: compute j=3,4 (positive), j=2=SCALE, j=0,1 by reciprocal
    let mut exp_az = [[0i128; 5]; 3];
    for i in 0..3 {
        exp_az[i][2] = SCALE_I; // zero node
        let arg3 = fp_mul_i(a_coeff[i], SQRT2_GH5[3])?;
        let arg4 = fp_mul_i(a_coeff[i], SQRT2_GH5[4])?;
        exp_az[i][3] = exp_fixed_i(arg3)?;
        exp_az[i][4] = exp_fixed_i(arg4)?;
        // Negative nodes by reciprocal: exp(-x) = SCALE²/exp(x)
        exp_az[i][1] = SCALE_SQ / exp_az[i][3];
        exp_az[i][0] = SCALE_SQ / exp_az[i][4];
    }

    // Table 2: exp_bz[i][k] — same structure
    let mut exp_bz = [[0i128; 5]; 3];
    for i in 0..3 {
        exp_bz[i][2] = SCALE_I;
        let arg3 = fp_mul_i(b_coeff[i], SQRT2_GH5[3])?;
        let arg4 = fp_mul_i(b_coeff[i], SQRT2_GH5[4])?;
        exp_bz[i][3] = exp_fixed_i(arg3)?;
        exp_bz[i][4] = exp_fixed_i(arg4)?;
        exp_bz[i][1] = SCALE_SQ / exp_bz[i][3];
        exp_bz[i][0] = SCALE_SQ / exp_bz[i][4];
    }

    // Table 3: exp_gamma[i][m] = exp(γ[i] × √2 × GH7_NODES[m])
    // where γ[i] = c_vec[i] × σ_S
    let gamma = [
        fp_mul_i(c_vec[0], sigma_s)?,
        fp_mul_i(c_vec[1], sigma_s)?,
        fp_mul_i(c_vec[2], sigma_s)?,
    ];
    let mut exp_gamma = [[0i128; 7]; 3];
    for i in 0..3 {
        exp_gamma[i][3] = SCALE_I; // zero node
        for m in 4..7 {
            exp_gamma[i][m] = exp_fixed_i(fp_mul_i(gamma[i], SQRT2_GH7[m])?)?;
        }
        for m in 0..3 {
            exp_gamma[i][m] = SCALE_SQ / exp_gamma[i][6 - m];
        }
    }

    // Table 4: exp_sigma[m] = exp(σ_S × √2 × GH7_NODES[m])
    let mut exp_sigma = [0i128; 7];
    exp_sigma[3] = SCALE_I;
    for m in 4..7 {
        exp_sigma[m] = exp_fixed_i(fp_mul_i(sigma_s, SQRT2_GH7[m])?)?;
    }
    for m in 0..3 {
        exp_sigma[m] = SCALE_SQ / exp_sigma[6 - m];
    }

    // Fixed values
    let exp_mu = [
        exp_fixed_i(mu[0])?,
        exp_fixed_i(mu[1])?,
        exp_fixed_i(mu[2])?,
    ];
    let exp_mu_s = exp_fixed_i(mu_s)?;

    // dY precomputation: dY[i][j][k] = a[i]×√2×x_j + b[i]×√2×x_k
    let mut dy = [[[0i128; 5]; 5]; 3];
    for i in 0..3 {
        for j in 0..5 {
            let a_term = fp_mul_i(a_coeff[i], SQRT2_GH5[j])?;
            for k in 0..5 {
                dy[i][j][k] = a_term + fp_mul_i(b_coeff[i], SQRT2_GH5[k])?;
            }
        }
    }

    // ═══ Step 3: Analytical term (~250K CU) ═════════════════════════════════
    //
    // Full 3×3 eigendecomposition: L = chol(Σ), A = Lᵀ M L, eigen(A).
    // 3D analytical: GL7 over W₁ × GH3 over W₃ × chi2_call over W₂.

    let ef = analytical_eigen_full(&sigma_mat, w, &mu)?;
    let analytical = spread_split_3d(
        ef.lam1, ef.lam2, ef.lam3, ef.del1, ef.del2, ef.del3, ef.q0, d, c,
    )?;

    // ═══ Step 4: Correction loop — 7×5×5 (~390K CU) ════════════════════════

    let mut correction = 0i128;

    for i_out in 0..7 {
        // Outer node: exp(s) and exp(μ_cond) from precomputed tables
        let exp_s = fp_mul_i(exp_mu_s, exp_sigma[i_out])?;
        let p = exp_s;

        let exp_mc = [
            fp_mul_i(exp_mu[0], exp_gamma[0][i_out])?,
            fp_mul_i(exp_mu[1], exp_gamma[1][i_out])?,
            fp_mul_i(exp_mu[2], exp_gamma[2][i_out])?,
        ];

        // s value for Q computation
        let s = mu_s + fp_mul_i(sigma_s, SQRT2_GH7[i_out])?;
        let s_sq = fp_mul_i(s, s)?;

        // Conditional means (for Y, not exp)
        let ds = s - mu_s;
        let mu_cond = [
            mu[0] + fp_mul_i(c_vec[0], ds)?,
            mu[1] + fp_mul_i(c_vec[1], ds)?,
            mu[2] + fp_mul_i(c_vec[2], ds)?,
        ];

        for j in 0..5 {
            for k in 0..5 {
                // ─── H (hold portfolio) ───
                let exp_y0 = fp_mul_i(fp_mul_i(exp_mc[0], exp_az[0][j])?, exp_bz[0][k])?;
                let exp_y1 = fp_mul_i(fp_mul_i(exp_mc[1], exp_az[1][j])?, exp_bz[1][k])?;
                let exp_y2 = fp_mul_i(fp_mul_i(exp_mc[2], exp_az[2][j])?, exp_bz[2][k])?;
                let h = fp_mul_i(w[0], exp_y0)?
                    + fp_mul_i(w[1], exp_y1)?
                    + fp_mul_i(w[2], exp_y2)?
                    + pool.w_stable;
                let il = h - p;

                // ─── Q_full = ½[Σ wᵢYᵢ² − s²] ───
                let y0 = mu_cond[0] + dy[0][j][k];
                let y1 = mu_cond[1] + dy[1][j][k];
                let y2 = mu_cond[2] + dy[2][j][k];
                let w_y_sq = fp_mul_i(w[0], fp_mul_i(y0, y0)?)?
                    + fp_mul_i(w[1], fp_mul_i(y1, y1)?)?
                    + fp_mul_i(w[2], fp_mul_i(y2, y2)?)?;
                let q_val = (w_y_sq - s_sq) / 2;

                // ─── Correction payoff ───
                let il_d = (il - d).max(0);
                let q_d = (q_val - d).max(0);
                let il_c = (il - c).max(0);
                let q_c = (q_val - c).max(0);
                let payoff = (il_d - q_d) - (il_c - q_c);

                // ─── Accumulate ───
                let total_w = fp_mul_i(OUTER_W[i_out], INNER_W_FLAT[j * 5 + k])?;
                correction += fp_mul_i(total_w, payoff)?;
            }
        }
    }

    // ═══ Step 5: Result ════════════════════════════════════════════════════

    let premium = analytical + correction;
    let vault_premium = 2 * premium;
    Ok((premium, vault_premium))
}
