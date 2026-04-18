// N-token IL insurance premium.
//
// For N=3 this file uses the mathematically faithful nested quadrature path:
//   1. Outer conditioning on S = w^T Y
//   2. Residual-space factorization Y|S=s = m(s) + L x, x ~ N(0, I_2)
//   3. Direct capped payoff evaluation on a 2D inner Gauss-Hermite grid
//
// For N != 3 the legacy two-level Curran approximation is kept intact.

use crate::arithmetic::{fp_div_i, fp_mul_i, fp_mul_i_fast, fp_sqrt};
use crate::constants::*;
use crate::error::SolMathError;
use crate::gauss_hermite::{
    gh_rule, GH10_NODES, GH10_WEIGHTS, GH6_WEIGHTS, GH7_WEIGHTS, INV_SQRT_PI,
};
use crate::normal::norm_cdf_poly;
use crate::transcendental::exp_fixed_i;
use alloc::boxed::Box;
use alloc::vec;

/// Maximum number of tokens supported.
pub const MAX_TOKENS: usize = 8;

/// Number of outer GH nodes for the legacy Curran path.
const NUM_GH_NODES: usize = 10;

/// Default outer GH nodes for the N=3 nested quadrature path.
const N3_OUTER_GH_DEFAULT: usize = 7;

/// Default inner GH nodes per residual dimension for the N=3 nested quadrature path.
const N3_INNER_GH_DEFAULT: usize = 4;

/// Specialized outer GH order for the hot N=3 production path.
const N3_OUTER_GH_6: usize = 6;

/// Specialized outer GH order for the hot N=3 production path.
const N3_OUTER_GH_7: usize = 7;

/// Specialized inner GH order for the hot N=3 production path.
const N3_INNER_GH_6: usize = 6;

/// Specialized inner GH order for the hot N=3 production path.
const N3_INNER_GH_5: usize = 5;

/// Largest supported GH order in this module.
const MAX_GH_ORDER: usize = 10;
const N3_BASIS_MODE_SHIFT: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum N3BasisMode {
    Default,
    GradientAligned,
    AxisZeroLeastRow,
}

impl N3BasisMode {
    fn decode(inner_gh: usize) -> (usize, Self) {
        let mode = inner_gh / N3_BASIS_MODE_SHIFT;
        let inner = inner_gh % N3_BASIS_MODE_SHIFT;
        let basis_mode = match mode {
            0 => Self::Default,
            1 => Self::AxisZeroLeastRow,
            6 => Self::GradientAligned,
            _ => Self::Default,
        };
        (inner, basis_mode)
    }
}

const N3_7X5X5_OUTER_LEN: usize = 7;
const N3_7X5X5_INNER_LEN: usize = 5;
const N3_7X5X5_WEIGHT_PROD: usize = N3_7X5X5_INNER_LEN * N3_7X5X5_INNER_LEN;
const N3_6X6X6_OUTER_LEN: usize = 6;
const N3_6X6X6_INNER_LEN: usize = 6;
const N3_6X6X6_WEIGHT_PROD: usize = N3_6X6X6_INNER_LEN * N3_6X6X6_INNER_LEN;

/// Newton iterations for the legacy inner root-finding path.
const INNER_NEWTON_ITERS: usize = 3;

/// Guard against structurally degenerate inner auxiliary variables in Curran.
const INNER_SIGMA_G_SQ_TOL: i128 = 1_000;

/// Numerical tolerance for PSD checks on 2x2 residual covariance matrices.
const PSD_TOL: i128 = 1_000;

/// √2 at SCALE.
const SQRT2: i128 = 1_414_213_562_373;

/// 1/π at SCALE.
const INV_PI: i128 = 318_309_886_184;

/// Largest Gauss-Hermite weight reachable from the N=3 path.
///
/// `n3_nested_premium` accepts GH orders via `gh_rule`, but `MAX_GH_ORDER = 10`
/// rejects order 13 in both `build_inner_weight_products` and
/// `build_inner_exp_factors`. Across orders 3..10, the largest weight is the
/// center 3-point weight.
const MAX_SUPPORTED_INNER_GH_WEIGHT: i128 = 1_181_635_900_604;

/// `sqrt(2) * GH7_NODES[k]` at SCALE.
const N3_7X5X5_OUTER_Z: [i128; N3_7X5X5_OUTER_LEN] = [
    -3_750_439_717_725,
    -2_366_758_080_800,
    -1_154_395_514_288,
    0,
    1_154_395_514_288,
    2_366_758_080_800,
    3_750_439_717_725,
];

/// `sqrt(2) * GH6_NODES[k]` at SCALE.
const N3_6X6X6_OUTER_Z: [i128; N3_6X6X6_OUTER_LEN] = [
    -3_324_257_433_551,
    -1_889_175_877_754,
    -616_706_590_193,
    616_706_590_193,
    1_889_175_877_754,
    3_324_257_433_551,
];

/// `sqrt(2) * GH5_NODES[a]` at SCALE.
const N3_7X5X5_INNER_SCALED_NODES: [i128; N3_7X5X5_INNER_LEN] = [
    -2_856_970_013_872,
    -1_355_626_179_974,
    0,
    1_355_626_179_974,
    2_856_970_013_872,
];

/// `sqrt(2) * GH6_NODES[a]` at SCALE.
const N3_6X6X6_INNER_SCALED_NODES: [i128; N3_6X6X6_INNER_LEN] = [
    -3_324_257_433_551,
    -1_889_175_877_754,
    -616_706_590_193,
    616_706_590_193,
    1_889_175_877_754,
    3_324_257_433_551,
];

/// `GH5_WEIGHTS[a] * GH5_WEIGHTS[b] / SCALE` at SCALE.
const N3_7X5X5_WEIGHT_PROD_TABLE: [i128; N3_7X5X5_WEIGHT_PROD] = [
    398_131_906,
    7_854_111_425,
    18_860_943_732,
    7_854_111_425,
    398_131_906,
    7_854_111_425,
    154_936_169_597,
    372_085_830_069,
    154_936_169_597,
    7_854_111_425,
    18_860_943_732,
    372_085_830_069,
    893_607_578_264,
    372_085_830_069,
    18_860_943_732,
    7_854_111_425,
    154_936_169_597,
    372_085_830_069,
    154_936_169_597,
    7_854_111_425,
    398_131_906,
    7_854_111_425,
    18_860_943_732,
    7_854_111_425,
    398_131_906,
];

/// `GH6_WEIGHTS[a] * GH6_WEIGHTS[b] / SCALE` at SCALE.
const N3_6X6X6_WEIGHT_PROD_TABLE: [i128; N3_6X6X6_WEIGHT_PROD] = [
    20_520_990,
    711_516_517,
    3_282_579_245,
    3_282_579_245,
    711_516_517,
    20_520_990,
    711_516_517,
    24_670_143_113,
    113_815_628_749,
    113_815_628_749,
    24_670_143_113,
    711_516_517,
    3_282_579_245,
    113_815_628_749,
    525_088_050_274,
    525_088_050_274,
    113_815_628_749,
    3_282_579_245,
    3_282_579_245,
    113_815_628_749,
    525_088_050_274,
    525_088_050_274,
    113_815_628_749,
    3_282_579_245,
    711_516_517,
    24_670_143_113,
    113_815_628_749,
    113_815_628_749,
    24_670_143_113,
    711_516_517,
    20_520_990,
    711_516_517,
    3_282_579_245,
    3_282_579_245,
    711_516_517,
    20_520_990,
];

/// Compute the N-token IL insurance premium.
///
/// Inputs are signed fixed-point values at SCALE = 1e12.
///
/// For `N=3`, this uses exact outer conditioning on the pool log-return and a
/// direct 2D residual Gauss-Hermite inner quadrature. For all other supported
/// `N`, the existing two-level Curran approximation is retained.
#[inline(never)]
pub fn curran_premium(
    weights: &[i128],
    sigmas: &[i128],
    rho_flat: &[i128],
    t: i128,
    deductible: i128,
    cap: i128,
) -> Result<i128, SolMathError> {
    validate_premium_inputs(weights, sigmas, rho_flat, deductible, cap)?;
    if t <= 0 || sigmas.iter().all(|&sigma| sigma == 0) {
        return Ok(0);
    }

    if weights.len() == 3 {
        return n3_nested_premium(
            weights,
            sigmas,
            rho_flat,
            t,
            deductible,
            cap,
            N3_OUTER_GH_DEFAULT,
            N3_INNER_GH_DEFAULT,
            N3BasisMode::GradientAligned,
        );
    }

    curran_legacy_premium(weights, sigmas, rho_flat, t, deductible, cap)
}

/// Configurable-node N=3 premium entrypoint for benchmarking and tuning.
///
/// This is intentionally narrow: it only accepts `N=3` and routes directly to
/// the nested quadrature engine with caller-supplied GH orders.
#[inline(never)]
pub fn curran_premium_n3_with_nodes(
    weights: &[i128],
    sigmas: &[i128],
    rho_flat: &[i128],
    t: i128,
    deductible: i128,
    cap: i128,
    outer_gh: usize,
    inner_gh: usize,
) -> Result<i128, SolMathError> {
    validate_premium_inputs(weights, sigmas, rho_flat, deductible, cap)?;
    if weights.len() != 3 {
        return Err(SolMathError::DomainError);
    }
    if t <= 0 || sigmas.iter().all(|&sigma| sigma == 0) {
        return Ok(0);
    }
    let (inner_gh, basis_mode) = N3BasisMode::decode(inner_gh);
    curran_premium_n3_with_nodes_basis(
        weights, sigmas, rho_flat, t, deductible, cap, outer_gh, inner_gh, basis_mode,
    )
}

#[inline(never)]
pub fn curran_premium_n3_with_nodes_basis(
    weights: &[i128],
    sigmas: &[i128],
    rho_flat: &[i128],
    t: i128,
    deductible: i128,
    cap: i128,
    outer_gh: usize,
    inner_gh: usize,
    basis_mode: N3BasisMode,
) -> Result<i128, SolMathError> {
    validate_premium_inputs(weights, sigmas, rho_flat, deductible, cap)?;
    if weights.len() != 3 {
        return Err(SolMathError::DomainError);
    }
    if t <= 0 || sigmas.iter().all(|&sigma| sigma == 0) {
        return Ok(0);
    }
    n3_nested_premium(
        weights, sigmas, rho_flat, t, deductible, cap, outer_gh, inner_gh, basis_mode,
    )
}

fn validate_premium_inputs(
    weights: &[i128],
    sigmas: &[i128],
    rho_flat: &[i128],
    deductible: i128,
    cap: i128,
) -> Result<(), SolMathError> {
    let n = weights.len();
    if n < 2 || n > MAX_TOKENS {
        return Err(SolMathError::DomainError);
    }
    if sigmas.len() != n || rho_flat.len() != n * n {
        return Err(SolMathError::DomainError);
    }
    if deductible < 0 || cap <= deductible {
        return Err(SolMathError::DomainError);
    }

    let mut weight_sum = 0i128;
    for &w in weights {
        if w <= 0 {
            return Err(SolMathError::DomainError);
        }
        weight_sum += w;
    }
    if weight_sum != SCALE_I {
        return Err(SolMathError::DomainError);
    }

    Ok(())
}

#[inline(never)]
fn curran_legacy_premium(
    weights: &[i128],
    sigmas: &[i128],
    rho_flat: &[i128],
    t: i128,
    deductible: i128,
    cap: i128,
) -> Result<i128, SolMathError> {
    let n = weights.len();

    // Covariance matrix: Sig[i][j] = rho_ij * sigma_i * sigma_j * T
    let mut sig = [[0i128; MAX_TOKENS]; MAX_TOKENS];
    for i in 0..n {
        for j in 0..n {
            let rij = rho_flat[i * n + j];
            let si_sj = fp_mul_i(sigmas[i], sigmas[j])?;
            sig[i][j] = fp_mul_i(fp_mul_i(rij, si_sj)?, t)?;
        }
    }

    // mu_S = -sum(w_i * sigma_i^2 * T) / 2
    let mut mu_s: i128 = 0;
    for i in 0..n {
        let si_sq_t = fp_mul_i(fp_mul_i(sigmas[i], sigmas[i])?, t)?;
        mu_s -= fp_mul_i(weights[i], si_sq_t)?;
    }
    mu_s /= 2;

    // sigma_S^2 = sum_ij w_i w_j Sig_ij
    let mut sigma_s_sq: i128 = 0;
    for i in 0..n {
        for j in 0..n {
            sigma_s_sq += fp_mul_i(fp_mul_i(weights[i], weights[j])?, sig[i][j])?;
        }
    }
    if sigma_s_sq <= 0 {
        return Ok(0);
    }
    let sigma_s = fp_sqrt(sigma_s_sq as u128)? as i128;
    if sigma_s == 0 {
        return Ok(0);
    }

    // Conditioning coefficients: c_i, beta_i
    let mut ci = [0i128; MAX_TOKENS];
    let mut beta = [0i128; MAX_TOKENS];
    for i in 0..n {
        let mut sum_c: i128 = 0;
        for j in 0..n {
            sum_c += fp_mul_i(weights[j], sig[i][j])?;
        }
        ci[i] = sum_c;
        beta[i] = fp_div_i(sum_c, sigma_s)?;
    }

    // mu_i = -sigma_i^2 T / 2
    let mut mu = [0i128; MAX_TOKENS];
    for i in 0..n {
        mu[i] = -fp_mul_i(fp_mul_i(sigmas[i], sigmas[i])?, t)? / 2;
    }

    // Conditional variances and covariances
    let mut v = [0i128; MAX_TOKENS];
    let mut v_ij = [[0i128; MAX_TOKENS]; MAX_TOKENS];
    for i in 0..n {
        v[i] = sig[i][i] - fp_div_i(fp_mul_i(ci[i], ci[i])?, sigma_s_sq)?;
        for j in 0..n {
            v_ij[i][j] = sig[i][j] - fp_div_i(fp_mul_i(ci[i], ci[j])?, sigma_s_sq)?;
        }
    }

    // alpha_i = mu_i + v_i/2,  A_i = exp(alpha_i)
    let mut alpha = [0i128; MAX_TOKENS];
    let mut a_out = [0i128; MAX_TOKENS];
    for i in 0..n {
        alpha[i] = mu[i] + v[i] / 2;
        a_out[i] = exp_fixed_i(alpha[i])?;
    }

    let b = default_inner_coeffs(n);
    let (q, sigma_g_sq) = inner_covariance_projection(&v_ij, &b, n)?;
    if sigma_g_sq <= INNER_SIGMA_G_SQ_TOL {
        return Err(SolMathError::DegenerateVariance);
    }
    let sigma_g = fp_sqrt(sigma_g_sq as u128)? as i128;
    if sigma_g == 0 {
        return Err(SolMathError::DegenerateVariance);
    }

    let mut vtilde = [0i128; MAX_TOKENS];
    for i in 0..n {
        vtilde[i] = v[i] - fp_div_i(fp_mul_i(q[i], q[i])?, sigma_g_sq)?;
    }

    let mut m_g_0: i128 = 0;
    let mut beta_g: i128 = 0;
    for i in 0..n {
        m_g_0 += fp_mul_i(b[i], mu[i])?;
        beta_g += fp_mul_i(b[i], beta[i])?;
    }

    let mut s_g = [0i128; MAX_TOKENS];
    let mut s_z = [0i128; MAX_TOKENS];
    let mut a_tilde = [0i128; MAX_TOKENS];
    let mut phi_shift = [0i128; MAX_TOKENS];

    for i in 0..n {
        s_g[i] = fp_div_i(q[i], sigma_g_sq)?;
        s_z[i] = beta[i] - fp_mul_i(s_g[i], beta_g)?;

        let a_tilde_val = mu[i] + vtilde[i] / 2 - fp_mul_i(s_g[i], m_g_0)?;
        a_tilde[i] = exp_fixed_i(a_tilde_val)?;

        phi_shift[i] = fp_div_i(q[i], sigma_g)?;
    }

    let mut premium_sum: i128 = 0;

    for k in 0..NUM_GH_NODES {
        let u = fp_mul_i(SQRT2, GH10_NODES[k])?;

        let mut f_arr = [0i128; MAX_TOKENS];
        for i in 0..n {
            let exp_bu = exp_fixed_i(fp_mul_i(beta[i], u)?)?;
            f_arr[i] = fp_mul_i(a_out[i], exp_bu)?;
        }

        let p_u = exp_fixed_i(mu_s + fp_mul_i(sigma_s, u)?)?;

        let mut e_h: i128 = 0;
        for i in 0..n {
            e_h += fp_mul_i(weights[i], f_arr[i])?;
        }

        let m_g_u = m_g_0 + fp_mul_i(beta_g, u)?;

        let mut b_arr = [0i128; MAX_TOKENS];
        for i in 0..n {
            let exp_szu = exp_fixed_i(fp_mul_i(s_z[i], u)?)?;
            b_arr[i] = fp_mul_i(weights[i], fp_mul_i(a_tilde[i], exp_szu)?)?;
        }

        let k_d = p_u + deductible;
        let call_d = if e_h <= k_d {
            0
        } else {
            let g_star = find_inner_root(&b_arr, &s_g, k_d, m_g_u, n)?;
            eval_inner_call(weights, &f_arr, &phi_shift, sigma_g, m_g_u, g_star, k_d, n)?
        };

        let k_c = p_u + cap;
        let call_c = if e_h <= k_c {
            0
        } else {
            let g_star = find_inner_root(&b_arr, &s_g, k_c, m_g_u, n)?;
            eval_inner_call(weights, &f_arr, &phi_shift, sigma_g, m_g_u, g_star, k_c, n)?
        };

        let payoff = if call_d > call_c { call_d - call_c } else { 0 };
        premium_sum += fp_mul_i(GH10_WEIGHTS[k], payoff)?;
    }

    let premium = fp_mul_i(INV_SQRT_PI, premium_sum)?;
    Ok(if premium > 0 { premium } else { 0 })
}

/// N=3 nested quadrature engine.
///
/// Math:
/// - `S = w^T Y`
/// - `Y | S=s = m(s) + L x`, `x ~ N(0, I_2)`
/// - `premium = E_s[E_x[min(max(H(x;s) - (exp(s)+d), 0), c-d)]]`
///
/// `Sigma` is the unconditional covariance of `Y`, but the inner residual
/// quadrature lives under the conditional law `Y | S=s`. The residual loading
/// must therefore be built from the conditional covariance
///
/// `V = Sigma - (Sigma w)(Sigma w)^T / (w^T Sigma w)`,
///
/// and not from `Sigma` directly.
struct N3ConditionalStats {
    mu: [i128; 3],
    mu_s: i128,
    sigma_s: i128,
    mean_slope: [i128; 3],
    v: [[i128; 3]; 3],
}

struct N3InnerPrecomp {
    exp_u: Box<[i128]>,
    exp_v: Box<[i128]>,
    inner_weight_prod: Box<[i128]>,
    inner_len: usize,
}

struct N3InnerPrecomp7x5x5 {
    exp_u0: [i128; N3_7X5X5_INNER_LEN],
    exp_u1: [i128; N3_7X5X5_INNER_LEN],
    exp_u2: [i128; N3_7X5X5_INNER_LEN],
    exp_v0: [i128; N3_7X5X5_INNER_LEN],
    exp_v1: [i128; N3_7X5X5_INNER_LEN],
    exp_v2: [i128; N3_7X5X5_INNER_LEN],
    inner_weight_prod: [i128; N3_7X5X5_WEIGHT_PROD],
    axis_zero_row: u8,
}

struct N3InnerPrecomp6x6x6 {
    exp_u0: [i128; N3_6X6X6_INNER_LEN],
    exp_u1: [i128; N3_6X6X6_INNER_LEN],
    exp_u2: [i128; N3_6X6X6_INNER_LEN],
    exp_v0: [i128; N3_6X6X6_INNER_LEN],
    exp_v1: [i128; N3_6X6X6_INNER_LEN],
    exp_v2: [i128; N3_6X6X6_INNER_LEN],
    inner_weight_prod: [i128; N3_6X6X6_WEIGHT_PROD],
}

#[inline(never)]
fn n3_nested_premium(
    weights: &[i128],
    sigmas: &[i128],
    rho_flat: &[i128],
    t: i128,
    deductible: i128,
    cap: i128,
    outer_gh: usize,
    inner_gh: usize,
    basis_mode: N3BasisMode,
) -> Result<i128, SolMathError> {
    if outer_gh == N3_OUTER_GH_6 && inner_gh == N3_INNER_GH_6 {
        return n3_nested_premium_6x6x6(weights, sigmas, rho_flat, t, deductible, cap, basis_mode);
    }
    if outer_gh == N3_OUTER_GH_7 && inner_gh == N3_INNER_GH_5 {
        return n3_nested_premium_7x5x5(weights, sigmas, rho_flat, t, deductible, cap, basis_mode);
    }

    let (outer_nodes, outer_weights) = gh_rule(outer_gh).ok_or(SolMathError::DomainError)?;
    let (inner_nodes, inner_weights) = gh_rule(inner_gh).ok_or(SolMathError::DomainError)?;
    let cap_width = cap - deductible;
    if cap_width < 0 {
        return Err(SolMathError::DomainError);
    }

    let w = [weights[0], weights[1], weights[2]];
    let sigma = [sigmas[0], sigmas[1], sigmas[2]];
    let Some(stats) = build_n3_conditional_stats(&w, &sigma, rho_flat, t)? else {
        return Ok(0);
    };

    n3_price_from_stats(
        &w,
        deductible,
        cap_width,
        outer_nodes,
        outer_weights,
        inner_nodes,
        inner_weights,
        &stats,
        basis_mode,
    )
}

#[inline(never)]
fn n3_nested_premium_6x6x6(
    weights: &[i128],
    sigmas: &[i128],
    rho_flat: &[i128],
    t: i128,
    deductible: i128,
    cap: i128,
    basis_mode: N3BasisMode,
) -> Result<i128, SolMathError> {
    let cap_width = cap - deductible;
    if cap_width < 0 {
        return Err(SolMathError::DomainError);
    }

    let w = [weights[0], weights[1], weights[2]];
    let sigma = [sigmas[0], sigmas[1], sigmas[2]];
    let Some(stats) = build_n3_conditional_stats(&w, &sigma, rho_flat, t)? else {
        return Ok(0);
    };

    let precomp = n3_build_inner_precomp_6x6x6(&w, &stats, basis_mode)?;
    n3_integrate_outer_6x6x6(&w, deductible, cap_width, &stats, &precomp)
}

#[inline(never)]
fn n3_nested_premium_7x5x5(
    weights: &[i128],
    sigmas: &[i128],
    rho_flat: &[i128],
    t: i128,
    deductible: i128,
    cap: i128,
    basis_mode: N3BasisMode,
) -> Result<i128, SolMathError> {
    let cap_width = cap - deductible;
    if cap_width < 0 {
        return Err(SolMathError::DomainError);
    }

    let w = [weights[0], weights[1], weights[2]];
    let sigma = [sigmas[0], sigmas[1], sigmas[2]];
    let Some(stats) = build_n3_conditional_stats(&w, &sigma, rho_flat, t)? else {
        return Ok(0);
    };

    let precomp = n3_build_inner_precomp_7x5x5(&w, &stats, basis_mode)?;
    n3_integrate_outer_7x5x5(&w, deductible, cap_width, &stats, &precomp)
}

#[inline(never)]
fn build_n3_conditional_stats(
    w: &[i128; 3],
    sigma: &[i128; 3],
    rho_flat: &[i128],
    t: i128,
) -> Result<Option<N3ConditionalStats>, SolMathError> {
    let mut sig = [[0i128; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let rij = rho_flat[i * 3 + j];
            let si_sj = fp_mul_i(sigma[i], sigma[j])?;
            sig[i][j] = fp_mul_i(fp_mul_i(rij, si_sj)?, t)?;
        }
    }

    let mut mu = [0i128; 3];
    for i in 0..3 {
        mu[i] = -fp_mul_i(fp_mul_i(sigma[i], sigma[i])?, t)? / 2;
    }

    let mut mu_s = 0i128;
    for i in 0..3 {
        mu_s += fp_mul_i(w[i], mu[i])?;
    }

    let c_vec = sigma_times_weights(&sig, &w)?;
    let mut sigma_s_sq = 0i128;
    for i in 0..3 {
        sigma_s_sq += fp_mul_i(w[i], c_vec[i])?;
    }
    if sigma_s_sq < 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    if sigma_s_sq == 0 {
        return Ok(None);
    }
    let sigma_s = fp_sqrt(sigma_s_sq as u128)? as i128;
    if sigma_s == 0 {
        return Err(SolMathError::DegenerateVariance);
    }

    let inv_sigma_s_sq = fp_div_i(SCALE_I, sigma_s_sq)?;
    let mut mean_slope = [0i128; 3];
    for i in 0..3 {
        mean_slope[i] = fp_mul_i(c_vec[i], inv_sigma_s_sq)?;
    }

    // Conditional covariance given S = w^T Y.
    let mut v = [[0i128; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let proj = fp_mul_i(fp_mul_i(c_vec[i], c_vec[j])?, inv_sigma_s_sq)?;
            v[i][j] = sig[i][j] - proj;
        }
    }

    Ok(Some(N3ConditionalStats {
        mu,
        mu_s,
        sigma_s,
        mean_slope,
        v,
    }))
}

#[inline(never)]
fn n3_build_inner_precomp(
    w: &[i128; 3],
    outer_nodes: &[i128],
    outer_weights: &[i128],
    inner_nodes: &[i128],
    inner_weights: &[i128],
    stats: &N3ConditionalStats,
    basis_mode: N3BasisMode,
) -> Result<N3InnerPrecomp, SolMathError> {
    let l = n3_select_loading(w, stats, outer_nodes, outer_weights, false, basis_mode)?;
    let (exp_u, exp_v) = build_inner_exp_factors(&l, inner_nodes)?;
    let inner_weight_prod = build_inner_weight_products(inner_weights)?;

    Ok(N3InnerPrecomp {
        exp_u,
        exp_v,
        inner_weight_prod,
        inner_len: inner_nodes.len(),
    })
}

#[inline(never)]
fn n3_build_inner_precomp_6x6x6(
    w: &[i128; 3],
    stats: &N3ConditionalStats,
    basis_mode: N3BasisMode,
) -> Result<Box<N3InnerPrecomp6x6x6>, SolMathError> {
    let l = n3_select_loading(w, stats, &N3_6X6X6_OUTER_Z, &GH6_WEIGHTS, true, basis_mode)?;
    let (exp_u, exp_v) = build_inner_exp_factors_6x6x6(&l)?;
    let inner_weight_prod = N3_6X6X6_WEIGHT_PROD_TABLE;

    Ok(Box::new(N3InnerPrecomp6x6x6 {
        exp_u0: exp_u[0],
        exp_u1: exp_u[1],
        exp_u2: exp_u[2],
        exp_v0: exp_v[0],
        exp_v1: exp_v[1],
        exp_v2: exp_v[2],
        inner_weight_prod,
    }))
}

#[inline(never)]
fn n3_build_inner_precomp_7x5x5(
    w: &[i128; 3],
    stats: &N3ConditionalStats,
    basis_mode: N3BasisMode,
) -> Result<N3InnerPrecomp7x5x5, SolMathError> {
    let axis_zero_row = match basis_mode {
        N3BasisMode::AxisZeroLeastRow => least_base_row(w, stats) as u8,
        _ => 255,
    };
    let l = match basis_mode {
        N3BasisMode::Default => n3_default_loading(w, stats)?,
        N3BasisMode::GradientAligned => {
            n3_geometry_aligned_loading(w, stats, &N3_7X5X5_OUTER_Z, &GH7_WEIGHTS, true)?
        }
        N3BasisMode::AxisZeroLeastRow => n3_axis_zero_loading(w, stats, axis_zero_row as usize)?,
    };
    let (exp_u, exp_v) = build_inner_exp_factors_7x5x5(&l, axis_zero_row)?;
    let inner_weight_prod = N3_7X5X5_WEIGHT_PROD_TABLE;

    Ok(N3InnerPrecomp7x5x5 {
        exp_u0: exp_u[0],
        exp_u1: exp_u[1],
        exp_u2: exp_u[2],
        exp_v0: exp_v[0],
        exp_v1: exp_v[1],
        exp_v2: exp_v[2],
        inner_weight_prod,
        axis_zero_row,
    })
}

#[inline(never)]
fn n3_price_from_stats(
    w: &[i128; 3],
    deductible: i128,
    cap_width: i128,
    outer_nodes: &[i128],
    outer_weights: &[i128],
    inner_nodes: &[i128],
    inner_weights: &[i128],
    stats: &N3ConditionalStats,
    basis_mode: N3BasisMode,
) -> Result<i128, SolMathError> {
    let precomp = n3_build_inner_precomp(
        w,
        outer_nodes,
        outer_weights,
        inner_nodes,
        inner_weights,
        stats,
        basis_mode,
    )?;
    n3_integrate_outer(
        w,
        deductible,
        cap_width,
        outer_nodes,
        outer_weights,
        stats,
        &precomp,
    )
}

#[inline(never)]
fn n3_integrate_outer(
    w: &[i128; 3],
    deductible: i128,
    cap_width: i128,
    outer_nodes: &[i128],
    outer_weights: &[i128],
    stats: &N3ConditionalStats,
    precomp: &N3InnerPrecomp,
) -> Result<i128, SolMathError> {
    // m_i(s_k) = mu_i + mean_slope_i * (s_k - mu_S), so only the affine slope
    // term changes across outer nodes.
    let mut base_a = [0i128; 3];
    for i in 0..3 {
        base_a[i] = fp_mul_i(w[i], exp_fixed_i(stats.mu[i])?)?;
    }
    let exp_mu_s = exp_fixed_i(stats.mu_s)?;

    let mut premium_sum = 0i128;
    for k in 0..outer_nodes.len() {
        let inner_value = evaluate_outer_node(
            outer_nodes[k],
            deductible,
            cap_width,
            stats,
            &base_a,
            exp_mu_s,
            precomp,
        )?;
        premium_sum += fp_mul_i(outer_weights[k], inner_value)?;
    }

    let premium = fp_mul_i(INV_SQRT_PI, premium_sum)?;
    Ok(premium.max(0))
}

#[inline(never)]
fn n3_integrate_outer_6x6x6(
    w: &[i128; 3],
    deductible: i128,
    cap_width: i128,
    stats: &N3ConditionalStats,
    precomp: &N3InnerPrecomp6x6x6,
) -> Result<i128, SolMathError> {
    let mut base_a = [0i128; 3];
    for i in 0..3 {
        base_a[i] = fp_mul_i(w[i], exp_fixed_i(stats.mu[i])?)?;
    }
    let exp_mu_s = exp_fixed_i(stats.mu_s)?;

    let mut premium_sum = 0i128;
    for k in 0..N3_6X6X6_OUTER_LEN {
        let inner_value = evaluate_outer_node_6x6x6(
            N3_6X6X6_OUTER_Z[k],
            deductible,
            cap_width,
            stats,
            &base_a,
            exp_mu_s,
            precomp,
        )?;
        premium_sum += fp_mul_i(GH6_WEIGHTS[k], inner_value)?;
    }

    let premium = fp_mul_i(INV_SQRT_PI, premium_sum)?;
    Ok(premium.max(0))
}

#[inline(never)]
fn n3_integrate_outer_7x5x5(
    w: &[i128; 3],
    deductible: i128,
    cap_width: i128,
    stats: &N3ConditionalStats,
    precomp: &N3InnerPrecomp7x5x5,
) -> Result<i128, SolMathError> {
    let mut base_a = [0i128; 3];
    for i in 0..3 {
        base_a[i] = fp_mul_i(w[i], exp_fixed_i(stats.mu[i])?)?;
    }
    let exp_mu_s = exp_fixed_i(stats.mu_s)?;

    let mut premium_sum = 0i128;
    for k in 0..N3_7X5X5_OUTER_LEN {
        let inner_value = evaluate_outer_node_7x5x5(
            N3_7X5X5_OUTER_Z[k],
            deductible,
            cap_width,
            stats,
            &base_a,
            exp_mu_s,
            precomp,
        )?;
        premium_sum += fp_mul_i(GH7_WEIGHTS[k], inner_value)?;
    }

    let premium = fp_mul_i(INV_SQRT_PI, premium_sum)?;
    Ok(premium.max(0))
}

#[inline(never)]
fn evaluate_outer_node(
    outer_node: i128,
    deductible: i128,
    cap_width: i128,
    stats: &N3ConditionalStats,
    base_a: &[i128; 3],
    exp_mu_s: i128,
    precomp: &N3InnerPrecomp,
) -> Result<i128, SolMathError> {
    let z_k = fp_mul_i(SQRT2, outer_node)?;
    let delta_s = fp_mul_i(stats.sigma_s, z_k)?;

    let mut a_k = [0i128; 3];
    for i in 0..3 {
        let slope_exp = exp_fixed_i(fp_mul_i(stats.mean_slope[i], delta_s)?)?;
        a_k[i] = fp_mul_i(base_a[i], slope_exp)?;
    }

    let threshold = fp_mul_i(exp_mu_s, exp_fixed_i(delta_s)?)? + deductible;
    evaluate_inner_payoff(
        &a_k,
        &precomp.exp_u,
        &precomp.exp_v,
        &precomp.inner_weight_prod,
        precomp.inner_len,
        threshold,
        cap_width,
    )
}

#[inline(never)]
fn evaluate_outer_node_6x6x6(
    outer_z: i128,
    deductible: i128,
    cap_width: i128,
    stats: &N3ConditionalStats,
    base_a: &[i128; 3],
    exp_mu_s: i128,
    precomp: &N3InnerPrecomp6x6x6,
) -> Result<i128, SolMathError> {
    let delta_s = fp_mul_i(stats.sigma_s, outer_z)?;

    let mut a_k = [0i128; 3];
    for i in 0..3 {
        let slope_exp = exp_fixed_i(fp_mul_i(stats.mean_slope[i], delta_s)?)?;
        a_k[i] = fp_mul_i(base_a[i], slope_exp)?;
    }

    let threshold = fp_mul_i(exp_mu_s, exp_fixed_i(delta_s)?)? + deductible;
    evaluate_inner_payoff_6x6x6(
        &a_k,
        &precomp.exp_u0,
        &precomp.exp_u1,
        &precomp.exp_u2,
        &precomp.exp_v0,
        &precomp.exp_v1,
        &precomp.exp_v2,
        &precomp.inner_weight_prod,
        threshold,
        cap_width,
    )
}

#[inline(never)]
fn evaluate_outer_node_7x5x5(
    outer_z: i128,
    deductible: i128,
    cap_width: i128,
    stats: &N3ConditionalStats,
    base_a: &[i128; 3],
    exp_mu_s: i128,
    precomp: &N3InnerPrecomp7x5x5,
) -> Result<i128, SolMathError> {
    let delta_s = fp_mul_i(stats.sigma_s, outer_z)?;

    let mut a_k = [0i128; 3];
    for i in 0..3 {
        let slope_exp = exp_fixed_i(fp_mul_i(stats.mean_slope[i], delta_s)?)?;
        a_k[i] = fp_mul_i(base_a[i], slope_exp)?;
    }

    let threshold = fp_mul_i(exp_mu_s, exp_fixed_i(delta_s)?)? + deductible;
    evaluate_inner_payoff_7x5x5(
        &a_k,
        &precomp.exp_u0,
        &precomp.exp_u1,
        &precomp.exp_u2,
        &precomp.exp_v0,
        &precomp.exp_v1,
        &precomp.exp_v2,
        &precomp.inner_weight_prod,
        precomp.axis_zero_row,
        threshold,
        cap_width,
    )
}

#[inline(never)]
fn evaluate_inner_payoff(
    a_k: &[i128; 3],
    exp_u: &[i128],
    exp_v: &[i128],
    inner_weight_prod: &[i128],
    inner_len: usize,
    threshold: i128,
    cap_width: i128,
) -> Result<i128, SolMathError> {
    let mut inner_sum = 0i128;
    for a in 0..inner_len {
        // For fixed outer node k and residual index a, precombine
        // W_i,a^(k) = A_i^(k) * U_i,a so the innermost b-loop only needs
        // one multiply per token: W_i,a^(k) * V_i,b.
        let mut w_a = [0i128; 3];
        for i in 0..3 {
            w_a[i] = fp_mul_i(a_k[i], exp_u[i * inner_len + a])?;
        }

        for b in 0..inner_len {
            let h = dot3_scaled_nonneg(
                &w_a,
                [exp_v[b], exp_v[inner_len + b], exp_v[2 * inner_len + b]],
            )?;

            let raw = h - threshold;
            let payoff = if raw <= 0 {
                0
            } else if raw >= cap_width {
                cap_width
            } else {
                raw
            };

            inner_sum += fp_mul_i(inner_weight_prod[a * inner_len + b], payoff)?;
        }
    }

    fp_mul_i(INV_PI, inner_sum)
}

#[inline(never)]
fn evaluate_inner_payoff_6x6x6(
    a_k: &[i128; 3],
    exp_u0: &[i128; N3_6X6X6_INNER_LEN],
    exp_u1: &[i128; N3_6X6X6_INNER_LEN],
    exp_u2: &[i128; N3_6X6X6_INNER_LEN],
    exp_v0: &[i128; N3_6X6X6_INNER_LEN],
    exp_v1: &[i128; N3_6X6X6_INNER_LEN],
    exp_v2: &[i128; N3_6X6X6_INNER_LEN],
    inner_weight_prod: &[i128; N3_6X6X6_WEIGHT_PROD],
    threshold: i128,
    cap_width: i128,
) -> Result<i128, SolMathError> {
    let mut inner_sum = 0i128;
    for a in 0..N3_6X6X6_INNER_LEN {
        let w_a0 = fp_mul_i(a_k[0], exp_u0[a])?;
        let w_a1 = fp_mul_i(a_k[1], exp_u1[a])?;
        let w_a2 = fp_mul_i(a_k[2], exp_u2[a])?;

        for b in 0..N3_6X6X6_INNER_LEN {
            let h = fp_mul_i(w_a0, exp_v0[b])?
                + fp_mul_i(w_a1, exp_v1[b])?
                + fp_mul_i(w_a2, exp_v2[b])?;

            let raw = h - threshold;
            let payoff = if raw <= 0 {
                0
            } else if raw >= cap_width {
                cap_width
            } else {
                raw
            };

            inner_sum += fp_mul_i(inner_weight_prod[a * N3_6X6X6_INNER_LEN + b], payoff)?;
        }
    }

    fp_mul_i(INV_PI, inner_sum)
}

#[inline(never)]
fn evaluate_inner_payoff_7x5x5(
    a_k: &[i128; 3],
    exp_u0: &[i128; N3_7X5X5_INNER_LEN],
    exp_u1: &[i128; N3_7X5X5_INNER_LEN],
    exp_u2: &[i128; N3_7X5X5_INNER_LEN],
    exp_v0: &[i128; N3_7X5X5_INNER_LEN],
    exp_v1: &[i128; N3_7X5X5_INNER_LEN],
    exp_v2: &[i128; N3_7X5X5_INNER_LEN],
    inner_weight_prod: &[i128; N3_7X5X5_WEIGHT_PROD],
    axis_zero_row: u8,
    threshold: i128,
    cap_width: i128,
) -> Result<i128, SolMathError> {
    let mut inner_sum = 0i128;
    for a in 0..N3_7X5X5_INNER_LEN {
        let w_a0 = fp_mul_i(a_k[0], exp_u0[a])?;
        let w_a1 = fp_mul_i(a_k[1], exp_u1[a])?;
        let w_a2 = fp_mul_i(a_k[2], exp_u2[a])?;

        for b in 0..N3_7X5X5_INNER_LEN {
            let h = match axis_zero_row {
                0 => w_a0 + fp_mul_i(w_a1, exp_v1[b])? + fp_mul_i(w_a2, exp_v2[b])?,
                1 => fp_mul_i(w_a0, exp_v0[b])? + w_a1 + fp_mul_i(w_a2, exp_v2[b])?,
                2 => fp_mul_i(w_a0, exp_v0[b])? + fp_mul_i(w_a1, exp_v1[b])? + w_a2,
                _ => {
                    fp_mul_i(w_a0, exp_v0[b])?
                        + fp_mul_i(w_a1, exp_v1[b])?
                        + fp_mul_i(w_a2, exp_v2[b])?
                }
            };

            let raw = h - threshold;
            let payoff = if raw <= 0 {
                0
            } else if raw >= cap_width {
                cap_width
            } else {
                raw
            };

            inner_sum += fp_mul_i(inner_weight_prod[a * N3_7X5X5_INNER_LEN + b], payoff)?;
        }
    }

    fp_mul_i(INV_PI, inner_sum)
}

#[inline]
fn dot3_scaled_nonneg(lhs: &[i128; 3], rhs: [i128; 3]) -> Result<i128, SolMathError> {
    Ok(fp_mul_i(lhs[0], rhs[0])? + fp_mul_i(lhs[1], rhs[1])? + fp_mul_i(lhs[2], rhs[2])?)
}

/// Precompute `inner_weight_prod[a][b] = inner_weight[a] * inner_weight[b]`.
///
/// This removes one fixed-point multiply from every inner `(a, b)` node.
/// The unchecked fast multiply is safe here because the Gauss-Hermite weights
/// are tiny fixed constants at SCALE:
/// - the largest supported inner weight is `GH3_WEIGHTS[1] = 1_181_635_900_604`
/// - so `max_weight^2 < 1.4e24`
/// - and `1.4e24 << i128::MAX ≈ 1.7e38`
#[inline(never)]
fn build_inner_weight_products(inner_weights: &[i128]) -> Result<Box<[i128]>, SolMathError> {
    if inner_weights.len() > MAX_GH_ORDER {
        return Err(SolMathError::DomainError);
    }

    let inner_len = inner_weights.len();
    let mut prod = vec![0i128; inner_len * inner_len];
    for a in 0..inner_weights.len() {
        for b in 0..inner_weights.len() {
            debug_assert!(
                inner_weights[a] >= 0 && inner_weights[a] <= MAX_SUPPORTED_INNER_GH_WEIGHT
            );
            debug_assert!(
                inner_weights[b] >= 0 && inner_weights[b] <= MAX_SUPPORTED_INNER_GH_WEIGHT
            );
            debug_assert!(
                inner_weights[a].checked_mul(inner_weights[b]).is_some(),
                "GH weight product must fit i128 before SCALE division"
            );
            // Safe fast path: for supported N=3 GH rules, both weights are in
            // [0, 1_181_635_900_604], so
            // |a*b| <= 1_181_635_900_604^2 = 1_396_264_420_782_468_108_816
            // < i128::MAX ≈ 1.70e38. Dividing by SCALE keeps fixed-point
            // semantics identical to fp_mul_i.
            prod[a * inner_len + b] = fp_mul_i_fast(inner_weights[a], inner_weights[b]);
        }
    }
    Ok(prod.into_boxed_slice())
}

fn sigma_times_weights(sig: &[[i128; 3]; 3], w: &[i128; 3]) -> Result<[i128; 3], SolMathError> {
    let mut out = [0i128; 3];
    for i in 0..3 {
        let mut sum = 0i128;
        for j in 0..3 {
            sum += fp_mul_i(sig[i][j], w[j])?;
        }
        out[i] = sum;
    }
    Ok(out)
}

/// Construct `Q` with orthonormal columns spanning the plane orthogonal to `w`.
fn orthonormal_residual_basis(w: &[i128; 3]) -> Result<[[i128; 2]; 3], SolMathError> {
    let w_hat = normalize_vec3(*w)?;

    let mut axis = 0usize;
    let mut best = w_hat[0].abs();
    for (idx, val) in w_hat.iter().enumerate().skip(1) {
        if val.abs() < best {
            best = val.abs();
            axis = idx;
        }
    }

    let mut seed = [0i128; 3];
    seed[axis] = SCALE_I;

    let proj = dot3(&seed, &w_hat)?;
    let mut q1_raw = [0i128; 3];
    for i in 0..3 {
        q1_raw[i] = seed[i] - fp_mul_i(proj, w_hat[i])?;
    }
    let q1 = normalize_vec3(q1_raw)?;
    let q2 = normalize_vec3(cross3(&w_hat, &q1)?)?;

    Ok([[q1[0], q2[0]], [q1[1], q2[1]], [q1[2], q2[2]]])
}

/// Compute `C = Q^T V Q` in residual coordinates.
///
/// `V` must be the conditional covariance of `Y | S=s`, not the unconditional
/// covariance `Sigma`. Projecting `Sigma` directly generally overstates the
/// inner residual variance because `Q^T w = 0` does not imply `Q^T Sigma w = 0`.
fn residual_covariance_2d(
    q: &[[i128; 2]; 3],
    v: &[[i128; 3]; 3],
) -> Result<[[i128; 2]; 2], SolMathError> {
    let q_col0 = [q[0][0], q[1][0], q[2][0]];
    let q_col1 = [q[0][1], q[1][1], q[2][1]];
    let cols = [q_col0, q_col1];

    let mut c = [[0i128; 2]; 2];
    for a in 0..2 {
        for b in 0..2 {
            let mut sum = 0i128;
            for i in 0..3 {
                for j in 0..3 {
                    sum += fp_mul_i(cols[a][i], fp_mul_i(v[i][j], cols[b][j])?)?;
                }
            }
            c[a][b] = sum;
        }
    }
    Ok(c)
}

/// 2x2 lower Cholesky factor `R` with `C = R R^T`.
fn cholesky_2x2_psd(c: [[i128; 2]; 2]) -> Result<[[i128; 2]; 2], SolMathError> {
    let a = c[0][0];
    let b = (c[0][1] + c[1][0]) / 2;
    let d = c[1][1];

    if a < -PSD_TOL || d < -PSD_TOL {
        return Err(SolMathError::DegenerateVariance);
    }

    let a_pos = a.max(0);
    if a_pos == 0 {
        if b.abs() > PSD_TOL {
            return Err(SolMathError::DegenerateVariance);
        }
        let d_pos = d.max(0);
        let l11 = if d_pos == 0 {
            0
        } else {
            fp_sqrt(d_pos as u128)? as i128
        };
        return Ok([[0, 0], [0, l11]]);
    }

    let l00 = fp_sqrt(a_pos as u128)? as i128;
    let l10 = fp_div_i(b, l00)?;
    let rem = d - fp_mul_i(l10, l10)?;
    if rem < -PSD_TOL {
        return Err(SolMathError::DegenerateVariance);
    }
    let l11 = if rem <= 0 {
        0
    } else {
        fp_sqrt(rem as u128)? as i128
    };

    Ok([[l00, 0], [l10, l11]])
}

/// Build `L = Q R`, so `Y|S=s = m(s) + L x` with `x ~ N(0, I_2)`.
fn residual_loading(
    q: &[[i128; 2]; 3],
    r: &[[i128; 2]; 2],
) -> Result<[[i128; 2]; 3], SolMathError> {
    let mut l = [[0i128; 2]; 3];
    for i in 0..3 {
        l[i][0] = fp_mul_i(q[i][0], r[0][0])? + fp_mul_i(q[i][1], r[1][0])?;
        l[i][1] = fp_mul_i(q[i][1], r[1][1])?;
    }
    Ok(l)
}

fn n3_geometry_aligned_loading(
    w: &[i128; 3],
    stats: &N3ConditionalStats,
    outer_nodes: &[i128],
    outer_weights: &[i128],
    nodes_are_scaled: bool,
) -> Result<[[i128; 2]; 3], SolMathError> {
    let base_loading = n3_default_loading(w, stats)?;
    let rotation = n3_outer_gradient_rotation(
        &base_loading,
        w,
        stats,
        outer_nodes,
        outer_weights,
        nodes_are_scaled,
    )?;
    rotate_loading_2d(&base_loading, &rotation)
}

fn n3_default_loading(
    w: &[i128; 3],
    stats: &N3ConditionalStats,
) -> Result<[[i128; 2]; 3], SolMathError> {
    let q = orthonormal_residual_basis(w)?;
    let c_res = residual_covariance_2d(&q, &stats.v)?;
    let r = cholesky_2x2_psd(c_res)?;
    residual_loading(&q, &r)
}

fn least_base_row(w: &[i128; 3], stats: &N3ConditionalStats) -> usize {
    let mut best_idx = 0usize;
    let mut best_score = i128::MAX;
    for i in 0..3 {
        let score = fp_mul_i(w[i], exp_fixed_i(stats.mu[i]).unwrap_or(0)).unwrap_or(0);
        if score < best_score {
            best_score = score;
            best_idx = i;
        }
    }
    best_idx
}

fn orthonormal_residual_basis_axis_zero(
    w: &[i128; 3],
    row: usize,
) -> Result<[[i128; 2]; 3], SolMathError> {
    let w_hat = normalize_vec3(*w)?;
    let q2_raw = match row {
        0 => [0, -w_hat[2], w_hat[1]],
        1 => [w_hat[2], 0, -w_hat[0]],
        2 => [-w_hat[1], w_hat[0], 0],
        _ => return Err(SolMathError::DomainError),
    };
    let q2 = normalize_vec3(q2_raw)?;
    let q1 = normalize_vec3(cross3(&q2, &w_hat)?)?;
    Ok([[q1[0], q2[0]], [q1[1], q2[1]], [q1[2], q2[2]]])
}

fn n3_axis_zero_loading(
    w: &[i128; 3],
    stats: &N3ConditionalStats,
    row: usize,
) -> Result<[[i128; 2]; 3], SolMathError> {
    let q = orthonormal_residual_basis_axis_zero(w, row)?;
    let c_res = residual_covariance_2d(&q, &stats.v)?;
    let r = cholesky_2x2_psd(c_res)?;
    residual_loading(&q, &r)
}

fn n3_select_loading(
    w: &[i128; 3],
    stats: &N3ConditionalStats,
    outer_nodes: &[i128],
    outer_weights: &[i128],
    nodes_are_scaled: bool,
    basis_mode: N3BasisMode,
) -> Result<[[i128; 2]; 3], SolMathError> {
    match basis_mode {
        N3BasisMode::GradientAligned => {
            n3_geometry_aligned_loading(w, stats, outer_nodes, outer_weights, nodes_are_scaled)
        }
        _ => n3_default_loading(w, stats),
    }
}

fn n3_outer_gradient_rotation(
    loading: &[[i128; 2]; 3],
    w: &[i128; 3],
    stats: &N3ConditionalStats,
    outer_nodes: &[i128],
    outer_weights: &[i128],
    nodes_are_scaled: bool,
) -> Result<[[i128; 2]; 2], SolMathError> {
    if outer_nodes.len() != outer_weights.len() {
        return Err(SolMathError::DomainError);
    }

    let mut base_a = [0i128; 3];
    for i in 0..3 {
        base_a[i] = fp_mul_i(w[i], exp_fixed_i(stats.mu[i])?)?;
    }

    let mut gram = [[0i128; 2]; 2];
    for k in 0..outer_nodes.len() {
        let outer_z = if nodes_are_scaled {
            outer_nodes[k]
        } else {
            fp_mul_i(SQRT2, outer_nodes[k])?
        };
        let delta_s = fp_mul_i(stats.sigma_s, outer_z)?;

        let mut gradient = [0i128; 2];
        for i in 0..3 {
            let slope_exp = exp_fixed_i(fp_mul_i(stats.mean_slope[i], delta_s)?)?;
            let a_k = fp_mul_i(base_a[i], slope_exp)?;
            gradient[0] += fp_mul_i(a_k, loading[i][0])?;
            gradient[1] += fp_mul_i(a_k, loading[i][1])?;
        }

        let g00 = fp_mul_i(gradient[0], gradient[0])?;
        let g01 = fp_mul_i(gradient[0], gradient[1])?;
        let g11 = fp_mul_i(gradient[1], gradient[1])?;

        gram[0][0] += fp_mul_i(outer_weights[k], g00)?;
        gram[0][1] += fp_mul_i(outer_weights[k], g01)?;
        gram[1][0] += fp_mul_i(outer_weights[k], g01)?;
        gram[1][1] += fp_mul_i(outer_weights[k], g11)?;
    }

    dominant_eigen_rotation_2x2(gram)
}

fn dominant_eigen_rotation_2x2(gram: [[i128; 2]; 2]) -> Result<[[i128; 2]; 2], SolMathError> {
    let a = gram[0][0];
    let b = (gram[0][1] + gram[1][0]) / 2;
    let d = gram[1][1];

    let axis = if b.abs() <= 1 {
        if d > a {
            [0, SCALE_I]
        } else {
            [SCALE_I, 0]
        }
    } else {
        let delta = d - a;
        let delta_sq = fp_mul_i(delta, delta)?;
        let four_b_sq = 4 * fp_mul_i(b, b)?;
        let rad_sq = delta_sq + four_b_sq;
        let rad = fp_sqrt(rad_sq.max(0) as u128)? as i128;
        let lambda = (a + d + rad) / 2;
        let raw = [b, lambda - a];
        normalize_vec2(raw)?
    };

    Ok([[axis[0], -axis[1]], [axis[1], axis[0]]])
}

fn rotate_loading_2d(
    loading: &[[i128; 2]; 3],
    rotation: &[[i128; 2]; 2],
) -> Result<[[i128; 2]; 3], SolMathError> {
    let mut rotated = [[0i128; 2]; 3];
    for i in 0..3 {
        rotated[i][0] =
            fp_mul_i(loading[i][0], rotation[0][0])? + fp_mul_i(loading[i][1], rotation[1][0])?;
        rotated[i][1] =
            fp_mul_i(loading[i][0], rotation[0][1])? + fp_mul_i(loading[i][1], rotation[1][1])?;
    }
    Ok(rotated)
}

/// Precompute
/// `U[i][a] = exp(L[i][0] * sqrt(2) * node[a])` and
/// `V[i][b] = exp(L[i][1] * sqrt(2) * node[b])`.
///
/// The inner loop then reconstructs
/// `T[i][a][b] = U[i][a] * V[i][b]`,
/// halving the number of expensive exponentials during setup.
#[inline(never)]
fn build_inner_exp_factors(
    l: &[[i128; 2]; 3],
    inner_nodes: &[i128],
) -> Result<(Box<[i128]>, Box<[i128]>), SolMathError> {
    if inner_nodes.len() > MAX_GH_ORDER {
        return Err(SolMathError::DomainError);
    }

    let inner_len = inner_nodes.len();
    let mut scaled_nodes = vec![0i128; inner_len];
    let mut exp_u = vec![0i128; 3 * inner_len];
    let mut exp_v = vec![0i128; 3 * inner_len];
    for (idx, node) in inner_nodes.iter().enumerate() {
        scaled_nodes[idx] = fp_mul_i(SQRT2, *node)?;
    }

    for i in 0..3 {
        for a in 0..inner_len {
            exp_u[i * inner_len + a] = exp_fixed_i(fp_mul_i(l[i][0], scaled_nodes[a])?)?;
        }
        for b in 0..inner_len {
            exp_v[i * inner_len + b] = exp_fixed_i(fp_mul_i(l[i][1], scaled_nodes[b])?)?;
        }
    }

    Ok((exp_u.into_boxed_slice(), exp_v.into_boxed_slice()))
}

#[inline(never)]
fn build_inner_exp_factors_6x6x6(
    l: &[[i128; 2]; 3],
) -> Result<
    (
        [[i128; N3_6X6X6_INNER_LEN]; 3],
        [[i128; N3_6X6X6_INNER_LEN]; 3],
    ),
    SolMathError,
> {
    let mut exp_u = [[0i128; N3_6X6X6_INNER_LEN]; 3];
    let mut exp_v = [[0i128; N3_6X6X6_INNER_LEN]; 3];
    for a in 0..N3_6X6X6_INNER_LEN {
        for i in 0..3 {
            exp_u[i][a] = exp_fixed_i(fp_mul_i(l[i][0], N3_6X6X6_INNER_SCALED_NODES[a])?)?;
            exp_v[i][a] = exp_fixed_i(fp_mul_i(l[i][1], N3_6X6X6_INNER_SCALED_NODES[a])?)?;
        }
    }
    Ok((exp_u, exp_v))
}

#[inline(never)]
fn build_inner_exp_factors_7x5x5(
    l: &[[i128; 2]; 3],
    axis_zero_row: u8,
) -> Result<
    (
        [[i128; N3_7X5X5_INNER_LEN]; 3],
        [[i128; N3_7X5X5_INNER_LEN]; 3],
    ),
    SolMathError,
> {
    let mut exp_u = [[0i128; N3_7X5X5_INNER_LEN]; 3];
    let mut exp_v = [[0i128; N3_7X5X5_INNER_LEN]; 3];

    for i in 0..3 {
        for a in 0..N3_7X5X5_INNER_LEN {
            exp_u[i][a] = exp_fixed_i(fp_mul_i(l[i][0], N3_7X5X5_INNER_SCALED_NODES[a])?)?;
        }
        for b in 0..N3_7X5X5_INNER_LEN {
            exp_v[i][b] = if i == axis_zero_row as usize {
                SCALE_I
            } else {
                exp_fixed_i(fp_mul_i(l[i][1], N3_7X5X5_INNER_SCALED_NODES[b])?)?
            };
        }
    }

    Ok((exp_u, exp_v))
}

fn dot3(a: &[i128; 3], b: &[i128; 3]) -> Result<i128, SolMathError> {
    Ok(fp_mul_i(a[0], b[0])? + fp_mul_i(a[1], b[1])? + fp_mul_i(a[2], b[2])?)
}

fn normalize_vec3(v: [i128; 3]) -> Result<[i128; 3], SolMathError> {
    let norm_sq = dot3(&v, &v)?;
    if norm_sq <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let norm = fp_sqrt(norm_sq as u128)? as i128;
    if norm == 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let inv_norm = fp_div_i(SCALE_I, norm)?;
    Ok([
        fp_mul_i(v[0], inv_norm)?,
        fp_mul_i(v[1], inv_norm)?,
        fp_mul_i(v[2], inv_norm)?,
    ])
}

fn normalize_vec2(v: [i128; 2]) -> Result<[i128; 2], SolMathError> {
    let norm_sq = fp_mul_i(v[0], v[0])? + fp_mul_i(v[1], v[1])?;
    if norm_sq <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let norm = fp_sqrt(norm_sq as u128)? as i128;
    if norm == 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let inv_norm = fp_div_i(SCALE_I, norm)?;
    Ok([fp_mul_i(v[0], inv_norm)?, fp_mul_i(v[1], inv_norm)?])
}

fn cross3(a: &[i128; 3], b: &[i128; 3]) -> Result<[i128; 3], SolMathError> {
    Ok([
        fp_mul_i(a[1], b[2])? - fp_mul_i(a[2], b[1])?,
        fp_mul_i(a[2], b[0])? - fp_mul_i(a[0], b[2])?,
        fp_mul_i(a[0], b[1])? - fp_mul_i(a[1], b[0])?,
    ])
}

fn default_inner_coeffs(n: usize) -> [i128; MAX_TOKENS] {
    let mut b = [0i128; MAX_TOKENS];
    let eq = SCALE_I / n as i128;
    for bi in b.iter_mut().take(n) {
        *bi = eq;
    }
    b[n - 1] += SCALE_I - eq * n as i128;
    b
}

fn inner_covariance_projection(
    v_ij: &[[i128; MAX_TOKENS]; MAX_TOKENS],
    b: &[i128; MAX_TOKENS],
    n: usize,
) -> Result<([i128; MAX_TOKENS], i128), SolMathError> {
    let mut q = [0i128; MAX_TOKENS];
    for i in 0..n {
        let mut sum_q: i128 = 0;
        for j in 0..n {
            sum_q += fp_mul_i(b[j], v_ij[i][j])?;
        }
        q[i] = sum_q;
    }

    let mut sigma_g_sq: i128 = 0;
    for i in 0..n {
        sigma_g_sq += fp_mul_i(b[i], q[i])?;
    }

    Ok((q, sigma_g_sq))
}

fn find_inner_root(
    b: &[i128],
    s_g: &[i128],
    strike: i128,
    m_g_u: i128,
    n: usize,
) -> Result<i128, SolMathError> {
    let mut g = m_g_u;

    for _ in 0..INNER_NEWTON_ITERS {
        let mut h: i128 = -strike;
        let mut hp: i128 = 0;

        for i in 0..n {
            let e = exp_fixed_i(fp_mul_i(s_g[i], g)?)?;
            let term = fp_mul_i(b[i], e)?;
            h += term;
            hp += fp_mul_i(term, s_g[i])?;
        }

        if hp.abs() < 1 {
            break;
        }

        let step = fp_div_i(h, hp)?;
        g -= step;

        if step.abs() < 2 {
            break;
        }
    }

    Ok(g)
}

fn eval_inner_call(
    w: &[i128],
    f: &[i128],
    phi_shift: &[i128],
    sigma_g: i128,
    m_g_u: i128,
    g_star: i128,
    strike: i128,
    n: usize,
) -> Result<i128, SolMathError> {
    let d0 = fp_div_i(m_g_u - g_star, sigma_g)?;

    let mut call: i128 = 0;
    for i in 0..n {
        let d_i = d0 + phi_shift[i];
        call += fp_mul_i(w[i], fp_mul_i(f[i], norm_cdf_poly(d_i)?)?)?;
    }
    call -= fp_mul_i(strike, norm_cdf_poly(d0)?)?;

    Ok(if call > 0 { call } else { 0 })
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    extern crate std;
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;
    use std::println;

    const S: i128 = SCALE_I;

    fn identity_rho(n: usize) -> Vec<i128> {
        let mut rho = vec![0i128; n * n];
        for i in 0..n {
            rho[i * n + i] = S;
        }
        rho
    }

    fn fp(x: f64) -> i128 {
        (x * 1e12) as i128
    }

    fn from_fp(x: i128) -> f64 {
        x as f64 / 1e12
    }

    fn permute_n3_case(
        weights: &[i128; 3],
        sigmas: &[i128; 3],
        rho_flat: &[i128],
        perm: [usize; 3],
    ) -> ([i128; 3], [i128; 3], Vec<i128>) {
        let w = [weights[perm[0]], weights[perm[1]], weights[perm[2]]];
        let s = [sigmas[perm[0]], sigmas[perm[1]], sigmas[perm[2]]];
        let mut rho = vec![0i128; 9];
        for i in 0..3 {
            for j in 0..3 {
                rho[i * 3 + j] = rho_flat[perm[i] * 3 + perm[j]];
            }
        }
        (w, s, rho)
    }

    fn gh_rule_f64(order: usize) -> (&'static [i128], &'static [i128]) {
        gh_rule(order).expect("supported GH rule")
    }

    fn sigma_times_weights_f64(sig: &[[f64; 3]; 3], w: &[f64; 3]) -> [f64; 3] {
        let mut out = [0.0; 3];
        for i in 0..3 {
            for j in 0..3 {
                out[i] += sig[i][j] * w[j];
            }
        }
        out
    }

    fn basis_from_weights_f64(w: &[f64; 3]) -> [[f64; 2]; 3] {
        let norm = (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt();
        let w_hat = [w[0] / norm, w[1] / norm, w[2] / norm];
        let mut axis = 0usize;
        let mut best = w_hat[0].abs();
        for (idx, val) in w_hat.iter().enumerate().skip(1) {
            if val.abs() < best {
                best = val.abs();
                axis = idx;
            }
        }

        let mut seed = [0.0; 3];
        seed[axis] = 1.0;
        let proj = seed[0] * w_hat[0] + seed[1] * w_hat[1] + seed[2] * w_hat[2];
        let q1_raw = [
            seed[0] - proj * w_hat[0],
            seed[1] - proj * w_hat[1],
            seed[2] - proj * w_hat[2],
        ];
        let q1_norm =
            (q1_raw[0] * q1_raw[0] + q1_raw[1] * q1_raw[1] + q1_raw[2] * q1_raw[2]).sqrt();
        let q1 = [
            q1_raw[0] / q1_norm,
            q1_raw[1] / q1_norm,
            q1_raw[2] / q1_norm,
        ];

        let q2_raw = [
            w_hat[1] * q1[2] - w_hat[2] * q1[1],
            w_hat[2] * q1[0] - w_hat[0] * q1[2],
            w_hat[0] * q1[1] - w_hat[1] * q1[0],
        ];
        let q2_norm =
            (q2_raw[0] * q2_raw[0] + q2_raw[1] * q2_raw[1] + q2_raw[2] * q2_raw[2]).sqrt();
        let q2 = [
            q2_raw[0] / q2_norm,
            q2_raw[1] / q2_norm,
            q2_raw[2] / q2_norm,
        ];

        [[q1[0], q2[0]], [q1[1], q2[1]], [q1[2], q2[2]]]
    }

    fn project_cov_f64(q: &[[f64; 2]; 3], m: &[[f64; 3]; 3]) -> [[f64; 2]; 2] {
        let mut out = [[0.0; 2]; 2];
        for a in 0..2 {
            for b in 0..2 {
                for i in 0..3 {
                    for j in 0..3 {
                        out[a][b] += q[i][a] * m[i][j] * q[j][b];
                    }
                }
            }
        }
        out
    }

    fn geometry_aligned_loading_f64(
        w: &[f64; 3],
        mu: &[f64; 3],
        mean_slope: &[f64; 3],
        v: &[[f64; 3]; 3],
        outer_nodes: &[i128],
        outer_weights: &[i128],
    ) -> [[f64; 2]; 3] {
        let q = basis_from_weights_f64(w);
        let c_res = project_cov_f64(&q, v);

        let l00 = c_res[0][0].max(0.0).sqrt();
        let l10 = if l00 == 0.0 { 0.0 } else { c_res[1][0] / l00 };
        let l11_sq = (c_res[1][1] - l10 * l10).max(0.0);
        let l11 = l11_sq.sqrt();
        let base = [
            [q[0][0] * l00 + q[0][1] * l10, q[0][1] * l11],
            [q[1][0] * l00 + q[1][1] * l10, q[1][1] * l11],
            [q[2][0] * l00 + q[2][1] * l10, q[2][1] * l11],
        ];

        let mut gram = [[0.0; 2]; 2];
        for k in 0..outer_nodes.len() {
            let z = 2.0_f64.sqrt() * from_fp(outer_nodes[k]);
            let mut grad = [0.0; 2];
            for i in 0..3 {
                let a_k = w[i] * (mu[i] + mean_slope[i] * z).exp();
                grad[0] += a_k * base[i][0];
                grad[1] += a_k * base[i][1];
            }
            let wk = from_fp(outer_weights[k]);
            gram[0][0] += wk * grad[0] * grad[0];
            gram[0][1] += wk * grad[0] * grad[1];
            gram[1][0] += wk * grad[0] * grad[1];
            gram[1][1] += wk * grad[1] * grad[1];
        }

        let axis = if gram[0][1].abs() <= 1e-14 {
            if gram[1][1] > gram[0][0] {
                [0.0, 1.0]
            } else {
                [1.0, 0.0]
            }
        } else {
            let delta = gram[1][1] - gram[0][0];
            let rad = (delta * delta + 4.0 * gram[0][1] * gram[0][1]).sqrt();
            let lambda = 0.5 * (gram[0][0] + gram[1][1] + rad);
            let raw = [gram[0][1], lambda - gram[0][0]];
            let norm = (raw[0] * raw[0] + raw[1] * raw[1]).sqrt();
            [raw[0] / norm, raw[1] / norm]
        };
        let rotation = [[axis[0], -axis[1]], [axis[1], axis[0]]];

        let mut rotated = [[0.0; 2]; 3];
        for i in 0..3 {
            rotated[i][0] = base[i][0] * rotation[0][0] + base[i][1] * rotation[1][0];
            rotated[i][1] = base[i][0] * rotation[0][1] + base[i][1] * rotation[1][1];
        }
        rotated
    }

    fn ref_n3_nested_premium_f64(
        weights: &[i128; 3],
        sigmas: &[i128; 3],
        rho_flat: &[i128],
        t: i128,
        deductible: i128,
        cap: i128,
        outer_gh: usize,
        inner_gh: usize,
    ) -> f64 {
        let (outer_nodes, outer_weights) = gh_rule_f64(outer_gh);
        let (inner_nodes, inner_weights) = gh_rule_f64(inner_gh);

        let w = [
            from_fp(weights[0]),
            from_fp(weights[1]),
            from_fp(weights[2]),
        ];
        let sigma = [from_fp(sigmas[0]), from_fp(sigmas[1]), from_fp(sigmas[2])];
        let t_f = from_fp(t);
        let d_f = from_fp(deductible);
        let width_f = from_fp(cap - deductible);

        let mut sig = [[0f64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                sig[i][j] = from_fp(rho_flat[i * 3 + j]) * sigma[i] * sigma[j] * t_f;
            }
        }

        let mut mu = [0f64; 3];
        for i in 0..3 {
            mu[i] = -0.5 * sigma[i] * sigma[i] * t_f;
        }

        let mut mu_s = 0.0;
        for i in 0..3 {
            mu_s += w[i] * mu[i];
        }

        let c_vec = sigma_times_weights_f64(&sig, &w);
        let mut sigma_s_sq = 0.0;
        for i in 0..3 {
            sigma_s_sq += w[i] * c_vec[i];
        }
        if sigma_s_sq <= 0.0 {
            return 0.0;
        }
        let sigma_s = sigma_s_sq.sqrt();
        let mut v = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                v[i][j] = sig[i][j] - (c_vec[i] * c_vec[j] / sigma_s_sq);
            }
        }
        let mut mean_slope = [0.0; 3];
        for i in 0..3 {
            mean_slope[i] = c_vec[i] / sigma_s;
        }
        let l = geometry_aligned_loading_f64(&w, &mu, &mean_slope, &v, outer_nodes, outer_weights);

        let mut table = [[[0.0; MAX_GH_ORDER]; MAX_GH_ORDER]; 3];
        for i in 0..3 {
            for a in 0..inner_nodes.len() {
                let xa = 2.0_f64.sqrt() * from_fp(inner_nodes[a]);
                for b in 0..inner_nodes.len() {
                    let xb = 2.0_f64.sqrt() * from_fp(inner_nodes[b]);
                    table[i][a][b] = (l[i][0] * xa + l[i][1] * xb).exp();
                }
            }
        }

        let mut premium = 0.0;
        for k in 0..outer_nodes.len() {
            let z = 2.0_f64.sqrt() * from_fp(outer_nodes[k]);
            let s_k = mu_s + sigma_s * z;
            let mut a_k = [0.0; 3];
            for i in 0..3 {
                let m_i = mu[i] + (c_vec[i] / sigma_s_sq) * (s_k - mu_s);
                a_k[i] = w[i] * m_i.exp();
            }
            let threshold = s_k.exp() + d_f;

            let mut inner = 0.0;
            for a in 0..inner_nodes.len() {
                for b in 0..inner_nodes.len() {
                    let h =
                        a_k[0] * table[0][a][b] + a_k[1] * table[1][a][b] + a_k[2] * table[2][a][b];
                    let raw = h - threshold;
                    let payoff = raw.max(0.0).min(width_f);
                    inner += from_fp(inner_weights[a]) * from_fp(inner_weights[b]) * payoff;
                }
            }
            premium += from_fp(outer_weights[k]) * (inner / std::f64::consts::PI);
        }

        premium / std::f64::consts::PI.sqrt()
    }

    fn direct_3d_gh_reference_f64(
        weights: &[i128; 3],
        sigmas: &[i128; 3],
        rho_flat: &[i128],
        t: i128,
        deductible: i128,
        cap: i128,
        gh_n: usize,
    ) -> f64 {
        let (nodes, weights_gh) = gh_rule_f64(gh_n);
        let w = [
            from_fp(weights[0]),
            from_fp(weights[1]),
            from_fp(weights[2]),
        ];
        let sigma = [from_fp(sigmas[0]), from_fp(sigmas[1]), from_fp(sigmas[2])];
        let t_f = from_fp(t);
        let d_f = from_fp(deductible);
        let width_f = from_fp(cap - deductible);

        let mut sig = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                sig[i][j] = from_fp(rho_flat[i * 3 + j]) * sigma[i] * sigma[j] * t_f;
            }
        }

        let mu = [
            -0.5 * sigma[0] * sigma[0] * t_f,
            -0.5 * sigma[1] * sigma[1] * t_f,
            -0.5 * sigma[2] * sigma[2] * t_f,
        ];

        let l00 = sig[0][0].sqrt();
        let l10 = sig[1][0] / l00;
        let l11 = (sig[1][1] - l10 * l10).max(0.0).sqrt();
        let l20 = sig[2][0] / l00;
        let l21 = if l11 > 0.0 {
            (sig[2][1] - l20 * l10) / l11
        } else {
            0.0
        };
        let l22 = (sig[2][2] - l20 * l20 - l21 * l21).max(0.0).sqrt();

        let mut total = 0.0;
        let sqrt2 = 2.0_f64.sqrt();
        for a in 0..nodes.len() {
            let z0 = sqrt2 * from_fp(nodes[a]);
            for b in 0..nodes.len() {
                let z1 = sqrt2 * from_fp(nodes[b]);
                for c in 0..nodes.len() {
                    let z2 = sqrt2 * from_fp(nodes[c]);
                    let y0 = mu[0] + l00 * z0;
                    let y1 = mu[1] + l10 * z0 + l11 * z1;
                    let y2 = mu[2] + l20 * z0 + l21 * z1 + l22 * z2;
                    let h = w[0] * y0.exp() + w[1] * y1.exp() + w[2] * y2.exp();
                    let p = (w[0] * y0 + w[1] * y1 + w[2] * y2).exp();
                    let payoff = (h - p - d_f).max(0.0).min(width_f);
                    total += from_fp(weights_gh[a])
                        * from_fp(weights_gh[b])
                        * from_fp(weights_gh[c])
                        * payoff;
                }
            }
        }

        total / std::f64::consts::PI.powf(1.5)
    }

    fn percentile(mut xs: Vec<f64>, pct: f64) -> f64 {
        if xs.is_empty() {
            return 0.0;
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let idx = ((pct * xs.len() as f64).ceil() as usize).saturating_sub(1);
        xs[idx.min(xs.len() - 1)]
    }

    fn median(xs: Vec<f64>) -> f64 {
        percentile(xs, 0.5)
    }

    // ── N=2 ──

    #[test]
    fn n2_zero_deductible() {
        let w = [fp(0.4), fp(0.6)];
        let sigma = [fp(0.5), fp(0.5)];
        let rho = identity_rho(2);
        let t = fp(30.0 / 365.0);
        let premium = curran_premium(&w, &sigma, &rho, t, 0, fp(0.20)).unwrap();
        assert!(premium > 0, "premium should be > 0, got {premium}");
        assert!(premium < fp(0.20), "premium {premium} should be < cap");
    }

    #[test]
    fn n2_high_vol() {
        let w = [fp(0.4), fp(0.6)];
        let sigma = [fp(1.0), fp(1.0)];
        let rho = identity_rho(2);
        let t = fp(30.0 / 365.0);
        let d = fp(0.01);
        let c = fp(0.20);
        let premium = curran_premium(&w, &sigma, &rho, t, d, c).unwrap();
        assert!(premium > 0, "premium should be > 0, got {premium}");
        assert!(premium < c - d, "premium {premium} exceeds c-d");
    }

    #[test]
    fn n2_asymmetric() {
        let w = [fp(0.3), fp(0.7)];
        let sigma = [fp(0.5), fp(0.8)];
        let rho = identity_rho(2);
        let t = fp(30.0 / 365.0);
        let premium = curran_premium(&w, &sigma, &rho, t, 0, fp(0.20)).unwrap();
        assert!(premium > 0, "premium should be > 0, got {premium}");
    }

    #[test]
    fn n2_inner_b_avoids_w_direction_degeneracy() {
        let w = [fp(0.1), fp(0.9)];
        let sigma = [fp(0.4), fp(0.4)];
        let rho = identity_rho(2);
        let t = 19_178_082_191;

        let mut sig = [[0i128; MAX_TOKENS]; MAX_TOKENS];
        for i in 0..2 {
            for j in 0..2 {
                sig[i][j] = fp_mul_i(
                    fp_mul_i(rho[i * 2 + j], fp_mul_i(sigma[i], sigma[j]).unwrap()).unwrap(),
                    t,
                )
                .unwrap();
            }
        }

        let mut sigma_s_sq: i128 = 0;
        for i in 0..2 {
            for j in 0..2 {
                sigma_s_sq += fp_mul_i(fp_mul_i(w[i], w[j]).unwrap(), sig[i][j]).unwrap();
            }
        }

        let mut ci = [0i128; MAX_TOKENS];
        let mut v_ij = [[0i128; MAX_TOKENS]; MAX_TOKENS];
        for i in 0..2 {
            for j in 0..2 {
                ci[i] += fp_mul_i(w[j], sig[i][j]).unwrap();
            }
        }
        for i in 0..2 {
            for j in 0..2 {
                v_ij[i][j] =
                    sig[i][j] - fp_div_i(fp_mul_i(ci[i], ci[j]).unwrap(), sigma_s_sq).unwrap();
            }
        }

        let mut b_w = [0i128; MAX_TOKENS];
        b_w[..2].copy_from_slice(&w);
        let (_, sigma_g_sq_w) = inner_covariance_projection(&v_ij, &b_w, 2).unwrap();
        assert!(
            sigma_g_sq_w <= INNER_SIGMA_G_SQ_TOL,
            "expected near-zero sigma_G_sq with b=w, got {sigma_g_sq_w}"
        );

        let b_eq = default_inner_coeffs(2);
        let (_, sigma_g_sq_eq) = inner_covariance_projection(&v_ij, &b_eq, 2).unwrap();
        assert!(
            sigma_g_sq_eq > INNER_SIGMA_G_SQ_TOL,
            "expected positive sigma_G_sq with equal-weight b, got {sigma_g_sq_eq}"
        );
    }

    // ── N=3 nested path ──

    #[test]
    fn n3_sanity_bounds() {
        let w = [fp(0.2), fp(0.3), fp(0.5)];
        let sigma = [fp(0.5), fp(0.6), fp(0.4)];
        let rho = identity_rho(3);
        let t = fp(30.0 / 365.0);
        let d = fp(0.01);
        let c = fp(0.20);
        let premium = curran_premium(&w, &sigma, &rho, t, d, c).unwrap();
        assert!(premium >= 0);
        assert!(
            premium <= c - d,
            "premium {premium} exceeds cap width {}",
            c - d
        );
    }

    #[test]
    fn n3_zero_vol_zero_premium() {
        let w = [fp(0.2), fp(0.3), fp(0.5)];
        let sigma = [0, 0, 0];
        let rho = identity_rho(3);
        let t = fp(30.0 / 365.0);
        let premium = curran_premium(&w, &sigma, &rho, t, fp(0.01), fp(0.20)).unwrap();
        assert_eq!(premium, 0);
    }

    #[test]
    fn n3_equal_weights_equal_vols_permutation_invariant() {
        let w = [fp(1.0 / 3.0), fp(1.0 / 3.0), S - 2 * fp(1.0 / 3.0)];
        let sigma = [fp(0.5), fp(0.5), fp(0.5)];
        let rho = vec![
            S,
            fp(0.3),
            fp(0.6),
            fp(0.3),
            S,
            fp(0.4),
            fp(0.6),
            fp(0.4),
            S,
        ];
        let t = fp(60.0 / 365.0);
        let d = fp(0.01);
        let c = fp(0.18);

        let p0 = curran_premium(&w, &sigma, &rho, t, d, c).unwrap();
        let (w1, s1, rho1) = permute_n3_case(&w, &sigma, &rho, [1, 2, 0]);
        let p1 = curran_premium(&w1, &s1, &rho1, t, d, c).unwrap();
        assert!(
            (p0 - p1).abs() <= 10_000_000,
            "permuted symmetric case should match within rounding: {p0} vs {p1}"
        );
    }

    #[test]
    fn n3_regression_vs_higher_precision_reference() {
        let cases = [
            (
                [fp(0.2), fp(0.3), fp(0.5)],
                [fp(0.5), fp(0.6), fp(0.4)],
                vec![S, 0, 0, 0, S, 0, 0, 0, S],
                fp(30.0 / 365.0),
                fp(0.01),
                fp(0.20),
            ),
            (
                [fp(0.25), fp(0.35), fp(0.40)],
                [fp(0.9), fp(0.5), fp(0.7)],
                vec![
                    S,
                    fp(0.2),
                    fp(-0.1),
                    fp(0.2),
                    S,
                    fp(0.35),
                    fp(-0.1),
                    fp(0.35),
                    S,
                ],
                fp(45.0 / 365.0),
                fp(0.015),
                fp(0.22),
            ),
            (
                [fp(0.15), fp(0.25), fp(0.60)],
                [fp(0.35), fp(0.55), fp(0.8)],
                vec![
                    S,
                    fp(0.5),
                    fp(0.25),
                    fp(0.5),
                    S,
                    fp(0.4),
                    fp(0.25),
                    fp(0.4),
                    S,
                ],
                fp(90.0 / 365.0),
                fp(0.02),
                fp(0.25),
            ),
        ];

        for (idx, (w, sigma, rho, t, d, c)) in cases.iter().enumerate() {
            let model = curran_premium(w, sigma, rho, *t, *d, *c).unwrap();
            let reference = ref_n3_nested_premium_f64(w, sigma, rho, *t, *d, *c, 10, 7);
            let diff = (from_fp(model) - reference).abs();
            assert!(
                diff <= 7.5e-4,
                "case {idx}: model={} ref={} diff={diff}",
                from_fp(model),
                reference
            );
        }
    }

    #[test]
    fn n3_conditional_covariance_projection_differs_from_sigma_projection() {
        let w = [0.2_f64, 0.3_f64, 0.5_f64];
        let sigma = [0.5_f64, 0.6_f64, 0.4_f64];
        let rho = [
            [1.0_f64, 0.5_f64, 0.25_f64],
            [0.5_f64, 1.0_f64, 0.35_f64],
            [0.25_f64, 0.35_f64, 1.0_f64],
        ];
        let t = 30.0_f64 / 365.0_f64;

        let mut sig = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                sig[i][j] = rho[i][j] * sigma[i] * sigma[j] * t;
            }
        }
        let c_vec = sigma_times_weights_f64(&sig, &w);
        let sigma_s_sq = w[0] * c_vec[0] + w[1] * c_vec[1] + w[2] * c_vec[2];
        let q = basis_from_weights_f64(&w);

        let mut v = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                v[i][j] = sig[i][j] - (c_vec[i] * c_vec[j] / sigma_s_sq);
            }
        }

        let c_from_sigma = project_cov_f64(&q, &sig);
        let c_from_v = project_cov_f64(&q, &v);

        let q_sigma_w = [
            q[0][0] * c_vec[0] + q[1][0] * c_vec[1] + q[2][0] * c_vec[2],
            q[0][1] * c_vec[0] + q[1][1] * c_vec[1] + q[2][1] * c_vec[2],
        ];

        assert!(
            q_sigma_w[0].abs() > 1e-6 || q_sigma_w[1].abs() > 1e-6,
            "expected Q^T Sigma w to be non-zero"
        );

        let diff = (c_from_sigma[0][0] - c_from_v[0][0]).abs()
            + (c_from_sigma[0][1] - c_from_v[0][1]).abs()
            + (c_from_sigma[1][0] - c_from_v[1][0]).abs()
            + (c_from_sigma[1][1] - c_from_v[1][1]).abs();
        assert!(
            diff > 1e-6,
            "expected C(Q^T Sigma Q) != C(Q^T V Q), diff={diff}"
        );
    }

    #[test]
    fn n3_geometry_aligned_loading_preserves_conditional_covariance() {
        let w = [fp(0.2), fp(0.3), fp(0.5)];
        let sigma = [fp(0.5), fp(0.6), fp(0.4)];
        let rho = vec![
            S,
            fp(0.5),
            fp(0.25),
            fp(0.5),
            S,
            fp(0.35),
            fp(0.25),
            fp(0.35),
            S,
        ];
        let t = fp(30.0 / 365.0);
        let stats = build_n3_conditional_stats(&w, &sigma, &rho, t)
            .unwrap()
            .expect("non-degenerate stats");
        let (outer_nodes, outer_weights) = gh_rule(7).unwrap();
        let loading =
            n3_geometry_aligned_loading(&w, &stats, outer_nodes, outer_weights, false).unwrap();

        let mut ll_t = [[0i128; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                ll_t[i][j] = fp_mul_i(loading[i][0], loading[j][0]).unwrap()
                    + fp_mul_i(loading[i][1], loading[j][1]).unwrap();
            }
        }

        for i in 0..3 {
            for j in 0..3 {
                let diff = (ll_t[i][j] - stats.v[i][j]).abs();
                assert!(
                    diff <= 5_000_000,
                    "entry ({i},{j}) drifted: ll_t={} v={} diff={diff}",
                    ll_t[i][j],
                    stats.v[i][j]
                );
            }
        }
    }

    #[test]
    fn n3_higher_gh_nodes_now_stabilize_on_representative_cases() {
        let cases = [
            (
                [fp(0.15), fp(0.25), fp(0.60)],
                [fp(0.5), fp(0.6), fp(0.4)],
                vec![
                    S,
                    fp(0.5),
                    fp(0.25),
                    fp(0.5),
                    S,
                    fp(0.35),
                    fp(0.25),
                    fp(0.35),
                    S,
                ],
                fp(30.0 / 365.0),
                fp(0.01),
                fp(0.10),
            ),
            (
                [fp(0.2), fp(0.3), fp(0.5)],
                [fp(0.5), fp(0.6), fp(0.4)],
                vec![
                    S,
                    fp(0.5),
                    fp(0.25),
                    fp(0.5),
                    S,
                    fp(0.35),
                    fp(0.25),
                    fp(0.35),
                    S,
                ],
                fp(7.0 / 365.0),
                fp(0.01),
                fp(0.10),
            ),
        ];

        for (w, sigma, rho, t, d, c) in cases {
            let ref_hi = direct_3d_gh_reference_f64(&w, &sigma, &rho, t, d, c, 10);
            let p_744 = ref_n3_nested_premium_f64(&w, &sigma, &rho, t, d, c, 7, 4);
            let p_1077 = ref_n3_nested_premium_f64(&w, &sigma, &rho, t, d, c, 10, 7);
            let p_1010 = ref_n3_nested_premium_f64(&w, &sigma, &rho, t, d, c, 10, 10);

            let e_744 = (p_744 - ref_hi).abs();
            let e_1077 = (p_1077 - ref_hi).abs();
            let e_1010 = (p_1010 - ref_hi).abs();

            assert!(
                e_1077 <= e_744 * 1.05 || e_1010 <= e_744 * 1.05,
                "expected higher GH nodes to improve or stabilize accuracy: e744={e_744} e1077={e_1077} e1010={e_1010}"
            );
            assert!(
                e_1010 <= e_1077 * 1.10,
                "expected 10x10x10 nested quadrature not to materially destabilize vs 10x7x7: e1077={e_1077} e1010={e_1010}"
            );
        }
    }

    // ── N=4 ──

    #[test]
    fn n4_basic() {
        let w = [fp(0.1), fp(0.2), fp(0.3), fp(0.4)];
        let sigma = [fp(0.5); 4];
        let rho = identity_rho(4);
        let t = fp(30.0 / 365.0);
        let premium = curran_premium(&w, &sigma, &rho, t, fp(0.01), fp(0.20)).unwrap();
        assert!(premium > 0, "N=4 premium should be > 0, got {premium}");
    }

    // ── Edge cases ──

    #[test]
    fn zero_time() {
        let w = [S / 2, S / 2];
        let sigma = [fp(0.5); 2];
        assert_eq!(
            curran_premium(&w, &sigma, &identity_rho(2), 0, fp(0.01), fp(0.20)).unwrap(),
            0
        );
    }

    #[test]
    fn bad_inputs() {
        let w = [S / 2, S / 2];
        let sigma = [fp(0.5); 2];
        let rho = identity_rho(2);
        let t = fp(30.0 / 365.0);
        assert_eq!(
            curran_premium(&w, &sigma, &rho, t, fp(0.20), fp(0.10)),
            Err(SolMathError::DomainError)
        );
        assert_eq!(
            curran_premium(&[S], &[fp(0.5)], &vec![S], t, fp(0.01), fp(0.20)),
            Err(SolMathError::DomainError)
        );
        assert_eq!(
            curran_premium(&[fp(0.4), fp(0.4)], &sigma, &rho, t, fp(0.01), fp(0.20)),
            Err(SolMathError::DomainError)
        );
    }

    #[test]
    #[ignore]
    fn n3_accuracy_sweep_648_report_7x5x5_8x5x5_6x6x6_and_7x6x6() {
        #[derive(Clone)]
        struct Row {
            w: [i128; 3],
            sigma: [i128; 3],
            rho: Vec<i128>,
            tenor_days: i32,
            d: i128,
            c: i128,
            reference: f64,
            approx: f64,
            abs_err: f64,
            rel_err: f64,
        }

        let weight_sets = [
            [fp(0.20), fp(0.30), fp(0.50)],
            [fp(0.15), fp(0.25), fp(0.60)],
            [fp(1.0 / 3.0), fp(1.0 / 3.0), S - 2 * fp(1.0 / 3.0)],
            [fp(0.45), fp(0.35), fp(0.20)],
        ];
        let vol_sets = [
            [fp(0.3), fp(0.3), fp(0.3)],
            [fp(0.5), fp(0.6), fp(0.4)],
            [fp(0.9), fp(0.5), fp(0.7)],
        ];
        let corr_sets = [
            ("independent", vec![S, 0, 0, 0, S, 0, 0, 0, S]),
            (
                "positive",
                vec![
                    S,
                    fp(0.5),
                    fp(0.25),
                    fp(0.5),
                    S,
                    fp(0.35),
                    fp(0.25),
                    fp(0.35),
                    S,
                ],
            ),
            (
                "mixed",
                vec![
                    S,
                    fp(0.2),
                    fp(-0.1),
                    fp(0.2),
                    S,
                    fp(0.35),
                    fp(-0.1),
                    fp(0.35),
                    S,
                ],
            ),
        ];
        let tenors_days = [7_i32, 30_i32, 90_i32];
        let deductibles = [fp(0.00), fp(0.01), fp(0.03)];
        let caps = [fp(0.10), fp(0.20)];

        for &(outer_gh, inner_gh) in &[
            (7usize, 5usize),
            (8usize, 5usize),
            (6usize, 6usize),
            (7usize, 6usize),
        ] {
            let mut rows: Vec<Row> = Vec::new();
            for w in weight_sets {
                for sigma in vol_sets {
                    for (_corr_name, rho) in &corr_sets {
                        for tenor_days in tenors_days {
                            let t = fp(tenor_days as f64 / 365.0);
                            for d in deductibles {
                                for c in caps {
                                    let reference =
                                        direct_3d_gh_reference_f64(&w, &sigma, rho, t, d, c, 13);
                                    let approx = from_fp(
                                        curran_premium_n3_with_nodes(
                                            &w, &sigma, rho, t, d, c, outer_gh, inner_gh,
                                        )
                                        .unwrap(),
                                    );
                                    let abs_err = (approx - reference).abs();
                                    let rel_err = if reference > 0.0 {
                                        abs_err / reference.abs()
                                    } else {
                                        0.0
                                    };
                                    rows.push(Row {
                                        w,
                                        sigma,
                                        rho: rho.clone(),
                                        tenor_days,
                                        d,
                                        c,
                                        reference,
                                        approx,
                                        abs_err,
                                        rel_err,
                                    });
                                }
                            }
                        }
                    }
                }
            }

            assert_eq!(rows.len(), 648);

            let rels: Vec<f64> = rows.iter().map(|r| r.rel_err).collect();
            let abss: Vec<f64> = rows.iter().map(|r| r.abs_err).collect();

            let small: Vec<&Row> = rows.iter().filter(|r| r.reference < 1e-4).collect();
            let mat_1e4: Vec<&Row> = rows.iter().filter(|r| r.reference >= 1e-4).collect();
            let mat_5e4: Vec<&Row> = rows.iter().filter(|r| r.reference >= 5e-4).collect();
            let mat_1e3: Vec<&Row> = rows.iter().filter(|r| r.reference >= 1e-3).collect();

            let worst_rel = rows
                .iter()
                .max_by(|a, b| a.rel_err.partial_cmp(&b.rel_err).unwrap())
                .unwrap();
            let worst_abs = rows
                .iter()
                .max_by(|a, b| a.abs_err.partial_cmp(&b.abs_err).unwrap())
                .unwrap();

            println!(
                "{}x{}x{} over 648 cases vs direct 3D GH 13x13x13",
                outer_gh, inner_gh, inner_gh
            );
            println!(
                "headline: median_rel={:.4}% p95_rel={:.4}% max_rel={:.4}% median_abs={:.8e} max_abs={:.8e}",
                100.0 * median(rels.clone()),
                100.0 * percentile(rels.clone(), 0.95),
                100.0 * percentile(rels, 1.0),
                median(abss.clone()),
                percentile(abss, 1.0),
            );
            println!(
                "small premiums (<1e-4): count={} median_abs={:.8e} max_abs={:.8e}",
                small.len(),
                median(small.iter().map(|r| r.abs_err).collect()),
                percentile(small.iter().map(|r| r.abs_err).collect(), 1.0),
            );
            println!(
                "material >=1e-4: count={} median_rel={:.4}% p95_rel={:.4}%",
                mat_1e4.len(),
                100.0 * median(mat_1e4.iter().map(|r| r.rel_err).collect()),
                100.0 * percentile(mat_1e4.iter().map(|r| r.rel_err).collect(), 0.95),
            );
            println!(
                "material >=5e-4: count={} median_rel={:.4}% p95_rel={:.4}%",
                mat_5e4.len(),
                100.0 * median(mat_5e4.iter().map(|r| r.rel_err).collect()),
                100.0 * percentile(mat_5e4.iter().map(|r| r.rel_err).collect(), 0.95),
            );
            println!(
                "material >=1e-3: count={} median_rel={:.4}% p95_rel={:.4}%",
                mat_1e3.len(),
                100.0 * median(mat_1e3.iter().map(|r| r.rel_err).collect()),
                100.0 * percentile(mat_1e3.iter().map(|r| r.rel_err).collect(), 0.95),
            );
            println!(
                "worst_rel: w={:?} sigma={:?} tenor={}d d={:.4} c={:.4} ref={:.8e} approx={:.8e} rel={:.4}% abs={:.8e}",
                worst_rel.w.iter().map(|&x| from_fp(x)).collect::<Vec<_>>(),
                worst_rel.sigma.iter().map(|&x| from_fp(x)).collect::<Vec<_>>(),
                worst_rel.tenor_days,
                from_fp(worst_rel.d),
                from_fp(worst_rel.c),
                worst_rel.reference,
                worst_rel.approx,
                100.0 * worst_rel.rel_err,
                worst_rel.abs_err,
            );
            println!(
                "worst_abs: w={:?} sigma={:?} tenor={}d d={:.4} c={:.4} ref={:.8e} approx={:.8e} rel={:.4}% abs={:.8e}",
                worst_abs.w.iter().map(|&x| from_fp(x)).collect::<Vec<_>>(),
                worst_abs.sigma.iter().map(|&x| from_fp(x)).collect::<Vec<_>>(),
                worst_abs.tenor_days,
                from_fp(worst_abs.d),
                from_fp(worst_abs.c),
                worst_abs.reference,
                worst_abs.approx,
                100.0 * worst_abs.rel_err,
                worst_abs.abs_err,
            );
        }
    }
}
