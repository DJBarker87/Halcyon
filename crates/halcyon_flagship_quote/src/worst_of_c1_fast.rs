//! C1 fast path: single-component factored worst-of quote in pure i64.
//!
//! Zero heap allocation. No Vec, no GaussianUvState. Stack variables only.
//! Per-observation variance scaling via precomputed constants for each
//! of the 6 quarterly observation dates.

use solmath_core::nig_weights_table::{nig_importance_weights_9, GH9_NODES_S6};
use solmath_core::worst_of_ki_i64::{cholesky6, ki_moment_i64_gh3, AffineCoord6};
use solmath_core::PHI2_RESID_QQQ_IWM;
use solmath_core::PHI2_RESID_SPY_IWM;
use solmath_core::PHI2_RESID_SPY_QQQ;
use solmath_core::{
    bvn_cdf_i64, fp_div_i, fp_mul_i, fp_sqrt, norm_cdf_i64, norm_pdf_i64, triangle_probability_i64,
    RegionMoment6, SolMathError, TrianglePre64,
};

const S6: i64 = 1_000_000;
pub(crate) const S12: i128 = 1_000_000_000_000;
const SQRT2_S6: i64 = 1_414_214;
pub(crate) const CF_ALPHA_S12: i128 = 30_369_787_684_189;
pub(crate) const CF_BETA_S12: i128 = -4_253_775_293_079;
pub(crate) const CF_GAMMA_S12: i128 = 30_070_407_375_669;
pub(crate) const CF_DELTA_SCALE_S12: i128 = 116_985_997_577;
const N_OBS: usize = 6;
const FAIR_COUPON_BPS_S6_SCALE: i128 = 10_000_000_000;
const S12_TO_S6_DIVISOR: i128 = 1_000_000;
const SPY_QQQ_IWM_RESIDUAL_VARIANCE_DIAG_DAILY_S12: [i128; 3] =
    [8_222_389, 19_862_760, 21_036_297];

/// i64 multiply at SCALE_6. Product fits i64 for |a|,|b| ≤ ~3e6.
#[inline(always)]
fn m6r(a: i64, b: i64) -> i64 {
    a * b / 1_000_000
}

#[inline(always)]
fn round_div_i128(value: i128, divisor: i128) -> Result<i64, SolMathError> {
    if divisor == 0 {
        return Err(SolMathError::DivisionByZero);
    }
    let half = divisor / 2;
    let adjusted = if value >= 0 {
        value.checked_add(half).ok_or(SolMathError::Overflow)?
    } else {
        value.checked_sub(half).ok_or(SolMathError::Overflow)?
    };
    i64::try_from(adjusted / divisor).map_err(|_| SolMathError::Overflow)
}

#[inline(always)]
pub(crate) fn c1_fast_quote_from_components(
    notional_s6: i64,
    fair_coupon_s6: i64,
    redemption_pv_s6: i64,
    coupon_annuity_pv_s6: i64,
    knock_in_rate_s6: i64,
    autocall_rate_s6: i64,
) -> C1FastQuote {
    let fair_coupon_bps_s6 =
        ((fair_coupon_s6 as i128) * FAIR_COUPON_BPS_S6_SCALE / notional_s6 as i128) as i64;
    C1FastQuote {
        fair_coupon_bps_s6,
        zero_coupon_pv_s6: redemption_pv_s6,
        coupon_annuity_pv_s6: coupon_annuity_pv_s6,
        knock_in_rate_s6,
        autocall_rate_s6,
    }
}

pub fn spy_qqq_iwm_step_drift_inputs_s6(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    step_days: u32,
) -> Result<([i64; 2], i64), SolMathError> {
    if sigma_s6 <= 0 || step_days == 0 {
        return Err(SolMathError::DomainError);
    }

    let sigma_s6_i128 = sigma_s6 as i128;
    let sigma_sq_s12 = sigma_s6_i128
        .checked_mul(sigma_s6_i128)
        .ok_or(SolMathError::Overflow)?;
    let delta_scale_step = CF_DELTA_SCALE_S12
        .checked_mul(step_days as i128)
        .ok_or(SolMathError::Overflow)?;
    let delta_s12 = fp_mul_i(sigma_sq_s12, delta_scale_step)?;
    let drift_location_s12 = fp_div_i(fp_mul_i(delta_s12, -CF_BETA_S12)?, CF_GAMMA_S12)?;

    let alpha_sq_s12 = fp_mul_i(CF_ALPHA_S12, CF_ALPHA_S12)?;
    let mut drifts_s12 = [0i128; 3];
    for index in 0..3 {
        let loading_s12 = (cfg.loadings[index] as i128)
            .checked_mul(S12_TO_S6_DIVISOR)
            .ok_or(SolMathError::Overflow)?;
        let beta_plus_loading_s12 = CF_BETA_S12
            .checked_add(loading_s12)
            .ok_or(SolMathError::Overflow)?;
        let shifted_sq_s12 = alpha_sq_s12
            .checked_sub(fp_mul_i(beta_plus_loading_s12, beta_plus_loading_s12)?)
            .ok_or(SolMathError::Overflow)?;
        if shifted_sq_s12 <= 0 {
            return Err(SolMathError::DomainError);
        }
        let shifted_sqrt_s12 = fp_sqrt(shifted_sq_s12 as u128)? as i128;
        let common_term_s12 = fp_mul_i(drift_location_s12, loading_s12)?
            .checked_add(fp_mul_i(
                delta_s12,
                CF_GAMMA_S12
                    .checked_sub(shifted_sqrt_s12)
                    .ok_or(SolMathError::Overflow)?,
            )?)
            .ok_or(SolMathError::Overflow)?;
        let gaussian_term_s12 = SPY_QQQ_IWM_RESIDUAL_VARIANCE_DIAG_DAILY_S12[index]
            .checked_mul(step_days as i128)
            .ok_or(SolMathError::Overflow)?
            / 2;
        drifts_s12[index] = common_term_s12
            .checked_add(gaussian_term_s12)
            .ok_or(SolMathError::Overflow)?
            .checked_neg()
            .ok_or(SolMathError::Overflow)?;
    }

    let drift_diffs = [
        round_div_i128(
            drifts_s12[1]
                .checked_sub(drifts_s12[0])
                .ok_or(SolMathError::Overflow)?,
            S12_TO_S6_DIVISOR,
        )?,
        round_div_i128(
            drifts_s12[2]
                .checked_sub(drifts_s12[0])
                .ok_or(SolMathError::Overflow)?,
            S12_TO_S6_DIVISOR,
        )?,
    ];
    let drift_shift_s18 = cfg.loadings.iter().zip(drifts_s12.iter()).try_fold(
        0i128,
        |acc, (loading_s6, drift_s12)| {
            acc.checked_add(
                (*loading_s6 as i128)
                    .checked_mul(*drift_s12)
                    .ok_or(SolMathError::Overflow)?,
            )
            .ok_or(SolMathError::Overflow)
        },
    )?;
    let drift_shift_63 = round_div_i128(drift_shift_s18, S12)?;

    Ok((drift_diffs, drift_shift_63))
}

/// Precomputed Cholesky-based geometry for the GH3 triple-complement correction.
/// All fields at SCALE_6 unless noted.
#[derive(Debug, Clone, Copy)]
pub struct TripleCorrectionPre {
    /// Cholesky factors of the `(u, v)` covariance. Used for the raw-moment correction.
    pub l11: i64,
    pub l21: i64,
    pub l22: i64,
    /// α_k / β_k for each plane (slope of Y-bound as function of X).
    pub slope: [i64; 3],
    /// 1 / β_k for each plane, at S6. Used to compute intercept per node.
    pub inv_beta: [i64; 3],
    /// Whether the constraint on Y is an upper bound (β_k < 0 → complement is Y < bound).
    pub is_upper: [bool; 3],
}

/// GH3 nodes and weights for E[f(X)] where X ~ N(0,1).
/// Nodes: ±√3 and 0. Weights: 1/6, 2/3, 1/6.
const GH3_X_NODES_S6: [i64; 3] = [-1_732_051, 0, 1_732_051]; // ±√3 at S6
const GH3_X_WEIGHTS_S6: [i64; 3] = [166_667, 666_667, 166_667]; // at S6 (sum = S6)
const TRIANGLE_PAIR_RHO_63: [i64; 3] = [8_070, -509_610, -864_483];
const TRIANGLE_PAIR_INV_SQRT_1MRHO2_63: [i64; 3] = [1_000_033, 1_162_243, 1_989_410];
const NODE_STATE_EPS_S6: i64 = 100;

/// Build the triple correction geometry from Cholesky factors and half-plane normals.
///
/// `l11, l21, l22`: Cholesky of (u,v) covariance at S6.
/// `au, av`: half-plane normals at S6.
pub fn build_triple_correction_pre(
    l11: i64,
    l21: i64,
    l22: i64,
    au: &[i64; 3],
    av: &[i64; 3],
) -> TripleCorrectionPre {
    let mut slope = [0i64; 3];
    let mut inv_beta = [0i64; 3];
    let mut is_upper = [false; 3];
    for k in 0..3 {
        // α_k = (au_k × l11 + av_k × l21) / S6
        let alpha_k = (au[k] as i128 * l11 as i128 + av[k] as i128 * l21 as i128) / S6 as i128;
        // β_k = av_k × l22 / S6
        let beta_k = av[k] as i128 * l22 as i128 / S6 as i128;
        if beta_k == 0 {
            // Degenerate: constraint is purely on X. Treat as always-satisfied.
            slope[k] = 0;
            inv_beta[k] = 0;
            is_upper[k] = false;
        } else {
            // slope_k = α_k / β_k at S6
            slope[k] = (alpha_k * S6 as i128 / beta_k) as i64;
            // inv_beta_k = S6² / β_k (so that intercept = num × inv_beta / S6)
            inv_beta[k] = (S6 as i128 * S6 as i128 / beta_k) as i64;
            // Complement of {α×X + β×Y ≤ γ} is {Y > γ/β - (α/β)X} when β > 0
            // or {Y < γ/β - (α/β)X} when β < 0.
            is_upper[k] = beta_k < 0;
        }
    }
    TripleCorrectionPre {
        l11,
        l21,
        l22,
        slope,
        inv_beta,
        is_upper,
    }
}

/// Compute the triple-complement Φ₃ correction via GH3 1D quadrature.
///
/// `num_k` = rhs_k - E[w_k] at S6 (the numerator of z_k before dividing by σ_wk).
/// Returns the correction at S6: P(all three complement conditions hold).
#[inline(always)]
pub fn triple_complement_gh3(pre: &TripleCorrectionPre, num: [i64; 3]) -> i64 {
    // intercept_k = num_k × inv_beta_k / S6 = (rhs_k - E[w_k]) / β_k at S6
    let intercept = [
        (num[0] as i128 * pre.inv_beta[0] as i128 / S6 as i128) as i64,
        (num[1] as i128 * pre.inv_beta[1] as i128 / S6 as i128) as i64,
        (num[2] as i128 * pre.inv_beta[2] as i128 / S6 as i128) as i64,
    ];

    let mut total: i64 = 0;
    for i in 0..3 {
        let x = GH3_X_NODES_S6[i];
        let w = GH3_X_WEIGHTS_S6[i];

        // For each plane: bound_k = intercept_k - slope_k × x / S6
        let mut lower = -4 * S6; // -4σ floor
        let mut upper = 4 * S6; // +4σ ceiling
        for k in 0..3 {
            let bound = intercept[k] - pre.slope[k] as i128 as i64 * x / S6;
            if pre.is_upper[k] {
                // complement condition: Y < bound
                upper = upper.min(bound);
            } else {
                // complement condition: Y > bound
                lower = lower.max(bound);
            }
        }
        if lower >= upper {
            continue; // empty interval
        }
        // P(lower < Y < upper) = Φ(upper) - Φ(lower) where Y ~ N(0,1)
        let p = (norm_cdf_i64(upper * S6) - norm_cdf_i64(lower * S6)).max(0);
        total += m6r(w, p);
    }
    total.max(0)
}

/// Compute the triple-complement raw first moments via the same GH3 correction geometry.
///
/// Returns `(P, E[u 1_A], E[v 1_A])` at SCALE_6 for
/// `A = {w_1 > rhs_1, w_2 > rhs_2, w_3 > rhs_3}`.
#[inline(always)]
pub fn triple_complement_gh3_moment(
    pre: &TripleCorrectionPre,
    mean_u: i64,
    mean_v: i64,
    num: [i64; 3],
) -> (i64, i64, i64) {
    let intercept = [
        (num[0] as i128 * pre.inv_beta[0] as i128 / S6 as i128) as i64,
        (num[1] as i128 * pre.inv_beta[1] as i128 / S6 as i128) as i64,
        (num[2] as i128 * pre.inv_beta[2] as i128 / S6 as i128) as i64,
    ];

    let mut total_p: i64 = 0;
    let mut total_u: i64 = 0;
    let mut total_v: i64 = 0;
    for i in 0..3 {
        let x = GH3_X_NODES_S6[i];
        let w = GH3_X_WEIGHTS_S6[i];

        let mut lower = -4 * S6;
        let mut upper = 4 * S6;
        for k in 0..3 {
            let bound = intercept[k] - pre.slope[k] as i128 as i64 * x / S6;
            if pre.is_upper[k] {
                upper = upper.min(bound);
            } else {
                lower = lower.max(bound);
            }
        }
        if lower >= upper {
            continue;
        }

        let p = (norm_cdf_i64(upper * S6) - norm_cdf_i64(lower * S6)).max(0);
        if p == 0 {
            continue;
        }

        total_p += m6r(w, p);

        let u_x = mean_u + m6r(pre.l11, x);
        total_u += m6r(w, m6r(u_x, p));

        let v_x = mean_v + m6r(pre.l21, x);
        let pdf_delta = norm_pdf_i64(lower * S6) - norm_pdf_i64(upper * S6);
        let v_strip = m6r(v_x, p) + m6r(pre.l22, pdf_delta);
        total_v += m6r(w, v_strip);
    }

    (total_p.max(0), total_u, total_v)
}

/// Fused triangle probability + moments via inclusion-exclusion with
/// first-order truncated-normal corrections.
///
/// Lives in halcyon-quote (not solmath-core) to avoid BPF cross-crate
/// register-spill overhead (~8.3K CU per call). The primitives it calls
/// (norm_cdf_i64, norm_pdf_i64, bvn_cdf_i64) are small and inline fine.
///
/// `uncond_cov_s6`: unconditional `(Var_u, Cov_uv, Var_v)` at SCALE_6.
/// `triple_pre`: precomputed geometry for the GH3 Φ₃ correction. If `None`,
///   the 2-term I-E is used without correction (faster but overestimates small P).
/// Second moments use unconditional covariance scaled by region probability.
#[inline(always)]
pub fn triangle_probability_and_moments_local(
    mean_u: i128,
    mean_v: i128,
    rhs: [i128; 3],
    pre: &TrianglePre64,
    phi2_tables: [&[[i32; 64]; 64]; 3],
    cov_proj: &[[i64; 2]; 3],
    pair_rho: &[i64; 3],
    pair_inv_sqrt_1mrho2: &[i64; 3],
    uncond_cov_s6: [i64; 3], // [var_uu, cov_uv, var_vv] at S6
    triple_pre: Option<&TripleCorrectionPre>,
) -> RegionMoment6 {
    const SHIFT: i64 = 1_000_000;

    let mu6 = (mean_u / SHIFT as i128) as i64;
    let mv6 = (mean_v / SHIFT as i128) as i64;

    let mut num6_arr = [0i64; 3]; // saved for triple correction
    let mut rhs6_arr = [0i64; 3];

    for k in 0..3 {
        let rhs6 = (rhs[k] / SHIFT as i128) as i64;
        rhs6_arr[k] = rhs6;
        let ew6 = (pre.au[k] * mu6 + pre.av[k] * mv6) / S6;
        num6_arr[k] = rhs6 - ew6;
    }

    // In the fixed c1 geometry all three half-planes share the same shifted rhs.
    // When that common rhs is negative, the triangle is empty and the GH3 triple
    // correction can otherwise leave a small false-positive residual.
    if rhs6_arr[0] < 0 && rhs6_arr[1] == rhs6_arr[0] && rhs6_arr[2] == rhs6_arr[0] {
        return RegionMoment6::default();
    }

    let mut z_s6 = [0i64; 3];
    let mut z_scale = [0i64; 3];
    let mut phi_z = [0i64; 3];
    let mut pdf_z = [0i64; 3];
    for k in 0..3 {
        z_s6[k] = num6_arr[k] * pre.inv_std[k] / S6;
        z_scale[k] = z_s6[k] * SHIFT;
        phi_z[k] = norm_cdf_i64(z_scale[k]);
        pdf_z[k] = norm_pdf_i64(z_scale[k]);
    }

    let sum_complement = (S6 - phi_z[0]) + (S6 - phi_z[1]) + (S6 - phi_z[2]);

    let pairs: [(usize, usize); 3] = [(0, 1), (0, 2), (1, 2)];
    let mut sum_pair: i64 = 0;
    let mut pair_u_shift: i64 = 0;
    let mut pair_v_shift: i64 = 0;
    for (pidx, &(i, j)) in pairs.iter().enumerate() {
        let neg_zi = -z_scale[i];
        let neg_zj = -z_scale[j];
        let phi2 = if pre.phi2_neg[pidx] {
            let phi_a = norm_cdf_i64(neg_zi);
            (phi_a - bvn_cdf_i64(neg_zi, z_scale[j], phi2_tables[pidx])).max(0)
        } else {
            bvn_cdf_i64(neg_zi, neg_zj, phi2_tables[pidx])
        };
        sum_pair += phi2;

        let rho = pair_rho[pidx];
        let inv_sqrt = pair_inv_sqrt_1mrho2[pidx];
        let cond_i = m6r(m6r(rho, z_s6[i]) - z_s6[j], inv_sqrt);
        let cond_j = m6r(m6r(rho, z_s6[j]) - z_s6[i], inv_sqrt);
        let deriv_i = m6r(pdf_z[i], norm_cdf_i64(cond_i * SHIFT));
        let deriv_j = m6r(pdf_z[j], norm_cdf_i64(cond_j * SHIFT));
        pair_u_shift += m6r(cov_proj[i][0], deriv_i) + m6r(cov_proj[j][0], deriv_j);
        pair_v_shift += m6r(cov_proj[i][1], deriv_i) + m6r(cov_proj[j][1], deriv_j);
    }

    let prob_ie = (S6 - sum_complement + sum_pair).clamp(0, S6);

    // Apply the triple-complement correction to both probability and first raw moments.
    let (triple_p, triple_u, triple_v) = if let Some(tp) = triple_pre {
        triple_complement_gh3_moment(tp, mu6, mv6, num6_arr)
    } else {
        (0, 0, 0)
    };
    let prob = (prob_ie - triple_p).clamp(0, S6);
    if prob <= 0 {
        return RegionMoment6::default();
    }

    let mut single_u_shift: i64 = 0;
    let mut single_v_shift: i64 = 0;
    for k in 0..3 {
        single_u_shift += m6r(cov_proj[k][0], pdf_z[k]);
        single_v_shift += m6r(cov_proj[k][1], pdf_z[k]);
    }

    let eu = m6r(mu6, prob_ie) - single_u_shift + pair_u_shift - triple_u;
    let ev = m6r(mv6, prob_ie) - single_v_shift + pair_v_shift - triple_v;

    // Second moments use unconditional covariance scaled by region probability.
    let eu2 = (mu6 * mu6 / S6 + uncond_cov_s6[0]) * prob / S6;
    let euv = (mu6 * mv6 / S6 + uncond_cov_s6[1]) * prob / S6;
    let ev2 = (mv6 * mv6 / S6 + uncond_cov_s6[2]) * prob / S6;

    RegionMoment6 {
        probability: prob,
        expectation_u: eu,
        expectation_v: ev,
        expectation_uu: eu2,
        expectation_uv: euv,
        expectation_vv: ev2,
    }
}

/// Per-observation precomputed geometry (frozen, varies only with accumulated time).
pub struct ObsGeometry {
    pub tri_pre: TrianglePre64,
    pub cov_proj: [[i64; 2]; 3],
    pub cov_uu: i64,
    pub cov_uv: i64,
    pub cov_vv: i64,
    pub obs_day: u32,
}

/// All frozen constants for the c1 fast path.
pub struct C1FastConfig {
    pub obs: [ObsGeometry; N_OBS],
    pub loading_sum: i64,
    pub uv_slope: [i64; 2],
    pub loadings: [i64; 3],
    pub ki_barrier_log: i64,
    pub notional: i64,
    /// Half-plane au/av (same for all observations, stored once).
    pub au: [i64; 3],
    pub av: [i64; 3],
    /// Base rhs for autocall (=0) and KI safe barriers.
    pub autocall_rhs_base: i64,
    pub ki_safe_rhs_base: i64,
    /// Reserved for future per-name KI barrier shifts (BGK tested
    /// 2026-04-16, over-corrects for correlated worst-of ρ≈0.90).
    /// Shipping uses MC-retrained RBF to absorb continuous-KI gap instead.
    pub ki_bgk_shifts: Option<[i64; 3]>,
}

#[derive(Debug, Clone, Copy)]
pub struct C1FastQuote {
    pub fair_coupon_bps_s6: i64,
    pub zero_coupon_pv_s6: i64,
    pub coupon_annuity_pv_s6: i64,
    pub knock_in_rate_s6: i64,
    pub autocall_rate_s6: i64,
}

#[cfg(any(test, not(target_os = "solana")))]
impl C1FastQuote {
    #[inline(always)]
    pub fn fair_coupon_bps_f64(self) -> f64 {
        self.fair_coupon_bps_s6 as f64 / S6 as f64
    }

    #[inline(always)]
    pub fn zero_coupon_pv_f64(self) -> f64 {
        self.zero_coupon_pv_s6 as f64 / S6 as f64
    }

    #[inline(always)]
    pub fn coupon_annuity_pv_f64(self) -> f64 {
        self.coupon_annuity_pv_s6 as f64 / S6 as f64
    }

    #[inline(always)]
    pub fn knock_in_rate_f64(self) -> f64 {
        self.knock_in_rate_s6 as f64 / S6 as f64
    }

    #[inline(always)]
    pub fn autocall_rate_f64(self) -> f64 {
        self.autocall_rate_s6 as f64 / S6 as f64
    }
}

#[derive(Debug, Clone, Copy)]
struct C1FastTrace {
    quote: C1FastQuote,
    observation_survival: [i64; N_OBS],
    observation_autocall_first_hit: [i64; N_OBS],
}

#[derive(Debug, Clone, Copy, Default)]
struct NodeState {
    survival_w: i64,
    mean_u: i64,
    mean_v: i64,
}

/// C1 fast path: full 6-observation quote.
///
/// `sigma_s6`: σ_common at SCALE_6.
/// `drift_diffs`: `[drift[1]-drift[0], drift[2]-drift[0]]` at SCALE_6 for step=63.
/// `drift_shift_63`: `Σ l_i × drift_i` at SCALE_6 for step=63.
pub fn quote_c1_fast(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
) -> C1FastQuote {
    quote_c1_fast_trace(cfg, sigma_s6, drift_diffs, drift_shift_63).quote
}

fn quote_c1_fast_trace(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
) -> C1FastTrace {
    let phi2_tables: [&[[i32; 64]; 64]; 3] = [
        &PHI2_RESID_SPY_QQQ,
        &PHI2_RESID_SPY_IWM,
        &PHI2_RESID_QQQ_IWM,
    ];

    // NIG importance weights (9 table lookups, ~900 CU)
    let weights = nig_importance_weights_9(sigma_s6);

    // Factor node positions (same for all observations — 63-day step NIG)
    let proposal_std = sigma_s6 / 2; // √(63/252) = 0.5
    let mut factor_values = [0i64; 9];
    for k in 0..9 {
        factor_values[k] = SQRT2_S6 * proposal_std / S6 * GH9_NODES_S6[k] / S6;
    }

    let mut step_mean_u = [0i64; 9];
    let mut step_mean_v = [0i64; 9];
    let mut common_shift_step = [0i64; 9];
    let mut redemption_pv = 0i64;
    let mut coupon_annuity = 0i64;
    let mut total_ki = 0i64;
    let mut total_ac = 0i64;
    let mut observation_survival = [0i64; N_OBS];
    let mut observation_autocall_first_hit = [0i64; N_OBS];
    for k in 0..9 {
        step_mean_u[k] = drift_diffs[0] + cfg.uv_slope[0] * factor_values[k] / S6;
        step_mean_v[k] = drift_diffs[1] + cfg.uv_slope[1] * factor_values[k] / S6;
        common_shift_step[k] = factor_values[k] + drift_shift_63;
    }
    let triple_pre = core::array::from_fn::<_, N_OBS, _>(|i| {
        let obs = &cfg.obs[i];
        cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
            .ok()
            .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av))
    });
    let mut node_states = core::array::from_fn::<_, 9, _>(|k| NodeState {
        survival_w: weights[k],
        mean_u: 0,
        mean_v: 0,
    });

    for obs_idx in 0..N_OBS {
        let obs = &cfg.obs[obs_idx];
        let is_maturity = obs_idx + 1 == N_OBS;
        let coupon_count = (obs_idx + 1) as i64;
        // Scale factor: obs_day / 63 (how many 63-day steps accumulated)
        let scale = obs.obs_day as i64 / 63;

        let survival_mass = node_states
            .iter()
            .map(|state| state.survival_w)
            .sum::<i64>()
            .clamp(0, S6);
        observation_survival[obs_idx] = survival_mass;

        let mut obs_first_hit = 0i64;
        let mut obs_ki = 0i64;
        let mut obs_safe_principal = 0i64;
        let mut obs_ki_worst_ind = 0i64;
        let mut next_states = [NodeState::default(); 9];

        for k in 0..9 {
            let state = node_states[k];
            let fv = factor_values[k];
            let alive_w = state.survival_w;
            if alive_w <= 0 {
                continue;
            }

            // The node state stores the survivor mean up to the previous observation.
            // Each step adds one 63-day conditional increment before the current truncation.
            let mean_u = state.mean_u + step_mean_u[k];
            let mean_v = state.mean_v + step_mean_v[k];

            // Shifted rhs: base + scale × (fv + drift_shift_63)
            let shift = scale * common_shift_step[k];
            let ac_rhs = cfg.autocall_rhs_base + shift;
            let ki_rhs = cfg.ki_safe_rhs_base + shift;

            let ac_m = triangle_probability_and_moments_local(
                mean_u as i128 * S6 as i128,
                mean_v as i128 * S6 as i128,
                [ac_rhs as i128 * S6 as i128; 3],
                &obs.tri_pre,
                phi2_tables,
                &obs.cov_proj,
                &TRIANGLE_PAIR_RHO_63,
                &TRIANGLE_PAIR_INV_SQRT_1MRHO2_63,
                [obs.cov_uu, obs.cov_uv, obs.cov_vv],
                triple_pre[obs_idx].as_ref(),
            );

            let p_ki_safe = triangle_probability_i64(
                mean_u as i128 * S6 as i128,
                mean_v as i128 * S6 as i128,
                [ki_rhs as i128 * S6 as i128; 3],
                &obs.tri_pre,
                phi2_tables,
            ) / S6 as i128;

            let p_ac_k = ac_m.probability;
            let p_ki_safe_k = p_ki_safe as i64;
            obs_first_hit += m6r(alive_w, p_ac_k);
            obs_ki += m6r(alive_w, (S6 - p_ki_safe_k).max(0));
            obs_safe_principal += m6r(alive_w, p_ki_safe_k);

            if is_maturity {
                // KI moment
                let (l11, l21, l22) = match cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let ki_coords = ki_coords_from_factor(cfg, fv, drift_shift_63, scale);
                let ki_m =
                    ki_moment_i64_gh3(mean_u, mean_v, l11, l21, l22, cfg.ki_barrier_log, ki_coords);
                obs_ki_worst_ind += m6r(alive_w, ki_m.worst_indicator);
            } else {
                let survive_prob = (S6 - p_ac_k).max(0);
                let next_w = m6r(alive_w, survive_prob);
                if survive_prob > NODE_STATE_EPS_S6 && next_w > 0 {
                    let survive_mean_u = (((mean_u - ac_m.expectation_u) as i128) * S6 as i128
                        / survive_prob as i128) as i64;
                    let survive_mean_v = (((mean_v - ac_m.expectation_v) as i128) * S6 as i128
                        / survive_prob as i128) as i64;
                    next_states[k] = NodeState {
                        survival_w: next_w,
                        mean_u: survive_mean_u,
                        mean_v: survive_mean_v,
                    };
                }
            }
        }

        observation_autocall_first_hit[obs_idx] = obs_first_hit.clamp(0, survival_mass);

        if !is_maturity {
            redemption_pv += cfg.notional * obs_first_hit / S6;
            coupon_annuity += coupon_count * obs_first_hit;
            total_ac += obs_first_hit;
            total_ki += obs_ki;
            node_states = next_states;
        } else {
            redemption_pv += cfg.notional * obs_safe_principal / S6;
            redemption_pv += cfg.notional * obs_ki_worst_ind / S6;
            coupon_annuity += coupon_count * obs_first_hit;
            total_ki += obs_ki;
            total_ac += obs_first_hit;
        }
    }

    let loss = (cfg.notional - redemption_pv).max(0);
    let fair_coupon = if coupon_annuity > 100 {
        loss * S6 / coupon_annuity
    } else {
        0
    };
    let quote = c1_fast_quote_from_components(
        cfg.notional,
        fair_coupon,
        redemption_pv,
        coupon_annuity,
        total_ki,
        total_ac,
    );

    C1FastTrace {
        quote,
        observation_survival,
        observation_autocall_first_hit,
    }
}

#[inline(always)]
fn ki_coords_from_factor(
    cfg: &C1FastConfig,
    fv: i64,
    drift_shift_63: i64,
    scale: i64,
) -> [AffineCoord6; 3] {
    let l_sum = cfg.loading_sum;
    // constant = scale × (fv + drift_shift_63) / L_sum
    let spy_const = scale * (fv + drift_shift_63) * S6 / l_sum;
    let spy_u = -cfg.loadings[1] * S6 / l_sum;
    let spy_v = -cfg.loadings[2] * S6 / l_sum;
    [
        AffineCoord6 {
            constant: spy_const,
            u_coeff: spy_u,
            v_coeff: spy_v,
        },
        AffineCoord6 {
            constant: spy_const,
            u_coeff: S6 + spy_u,
            v_coeff: spy_v,
        },
        AffineCoord6 {
            constant: spy_const,
            u_coeff: spy_u,
            v_coeff: S6 + spy_v,
        },
    ]
}

/// Build the frozen config from the calibrated model constants.
pub fn spy_qqq_iwm_c1_config() -> C1FastConfig {
    let au = [567_972i64, -1_157_159, 567_972];
    let av = [641_427i64, 641_427, -1_083_704];

    let obs_data: [(u32, [i64; 3], [[i64; 2]; 3], i64, i64, i64); 6] = [
        (
            63,
            [25_468_814, 16_386_566, 15_922_934],
            [[22475, 41312], [-35191, 31655], [18982, -48003]],
            1756,
            -180,
            2688,
        ),
        (
            126,
            [18_009_171, 11_587_052, 11_259_214],
            [[31784, 58424], [-49768, 44766], [26844, -67887]],
            3513,
            -359,
            5376,
        ),
        (
            189,
            [14_704_427, 9_460_788, 9_193_110],
            [[38927, 71555], [-60952, 54827], [32877, -83144]],
            5269,
            -539,
            8063,
        ),
        (
            252,
            [12_734_407, 8_193_283, 7_961_467],
            [[44949, 82624], [-70382, 63309], [37964, -96007]],
            7026,
            -718,
            10751,
        ),
        (
            315,
            [11_390_000, 7_328_295, 7_120_952],
            [[50255, 92377], [-78689, 70782], [42445, -107339]],
            8782,
            -898,
            13439,
        ),
        (
            378,
            [10_397_600, 6_689_787, 6_500_510],
            [[55051, 101194], [-86200, 77538], [46496, -117584]],
            10538,
            -1077,
            16127,
        ),
    ];

    let obs = core::array::from_fn::<_, N_OBS, _>(|i| {
        let (day, inv_std, cov_proj, cuu, cuv, cvv) = obs_data[i];
        ObsGeometry {
            tri_pre: TrianglePre64 {
                au,
                av,
                inv_std,
                phi2_neg: [false, true, true],
            },
            cov_proj,
            cov_uu: cuu,
            cov_uv: cuv,
            cov_vv: cvv,
            obs_day: day,
        }
    });

    C1FastConfig {
        obs,
        loading_sum: 1_725_131,
        uv_slope: [52_241, 125_696],
        loadings: [515_731, 567_972, 641_427],
        ki_barrier_log: -223_144,
        notional: 100 * S6,
        au,
        av,
        autocall_rhs_base: 0,
        ki_safe_rhs_base: 384_952,
        ki_bgk_shifts: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worst_of_factored::FactoredWorstOfModel;

    fn c1_fast_inputs(sigma_common: f64) -> (C1FastConfig, [i64; 2], i64) {
        let cfg = spy_qqq_iwm_c1_config();
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let drifts = model.risk_neutral_step_drifts(sigma_common, 63).unwrap();
        let drift_diffs = [
            ((drifts[1] - drifts[0]) * S6 as f64).round() as i64,
            ((drifts[2] - drifts[0]) * S6 as f64).round() as i64,
        ];
        let drift_shift_63 = ((cfg.loadings[0] as f64 * drifts[0])
            + (cfg.loadings[1] as f64 * drifts[1])
            + (cfg.loadings[2] as f64 * drifts[2]))
            .round() as i64;
        (cfg, drift_diffs, drift_shift_63)
    }

    fn quote_c1_fast_trace_scalar(
        cfg: &C1FastConfig,
        sigma_s6: i64,
        drift_diffs: [i64; 2],
        drift_shift_63: i64,
    ) -> C1FastTrace {
        let phi2_tables: [&[[i32; 64]; 64]; 3] = [
            &PHI2_RESID_SPY_QQQ,
            &PHI2_RESID_SPY_IWM,
            &PHI2_RESID_QQQ_IWM,
        ];
        let weights = nig_importance_weights_9(sigma_s6);
        let proposal_std = sigma_s6 / 2;
        let mut factor_values = [0i64; 9];
        for k in 0..9 {
            factor_values[k] = SQRT2_S6 * proposal_std / S6 * GH9_NODES_S6[k] / S6;
        }

        let mut survival = S6;
        let mut redemption_pv = 0i64;
        let mut coupon_annuity = 0i64;
        let mut total_ki = 0i64;
        let mut total_ac = 0i64;
        let mut observation_survival = [0i64; N_OBS];
        let mut observation_autocall_first_hit = [0i64; N_OBS];

        for obs_idx in 0..N_OBS {
            let obs = &cfg.obs[obs_idx];
            let is_maturity = obs_idx + 1 == N_OBS;
            let coupon_count = (obs_idx + 1) as i64;
            let scale = obs.obs_day as i64 / 63;
            observation_survival[obs_idx] = survival;

            let mut obs_autocall = 0i64;
            let mut obs_ki_safe = 0i64;
            let mut obs_ki_worst_ind = 0i64;

            for k in 0..9 {
                let fv = factor_values[k];
                let w = weights[k];
                let mean_u = scale * (drift_diffs[0] + cfg.uv_slope[0] * fv / S6);
                let mean_v = scale * (drift_diffs[1] + cfg.uv_slope[1] * fv / S6);
                let shift = scale * (fv + drift_shift_63);
                let ac_rhs = cfg.autocall_rhs_base + shift;
                let ki_rhs = cfg.ki_safe_rhs_base + shift;

                let p_ac = triangle_probability_i64(
                    mean_u as i128 * S6 as i128,
                    mean_v as i128 * S6 as i128,
                    [ac_rhs as i128 * S6 as i128; 3],
                    &obs.tri_pre,
                    phi2_tables,
                ) / S6 as i128;
                let p_ki_safe = triangle_probability_i64(
                    mean_u as i128 * S6 as i128,
                    mean_v as i128 * S6 as i128,
                    [ki_rhs as i128 * S6 as i128; 3],
                    &obs.tri_pre,
                    phi2_tables,
                ) / S6 as i128;

                obs_autocall += w * p_ac as i64 / S6;
                obs_ki_safe += w * p_ki_safe as i64 / S6;

                if is_maturity {
                    let (l11, l21, l22) = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv).unwrap();
                    let ki_coords = ki_coords_from_factor(cfg, fv, drift_shift_63, scale);
                    let ki_m = ki_moment_i64_gh3(
                        mean_u,
                        mean_v,
                        l11,
                        l21,
                        l22,
                        cfg.ki_barrier_log,
                        ki_coords,
                    );
                    obs_ki_worst_ind += w * ki_m.worst_indicator / S6;
                }
            }

            let obs_ki = (S6 - obs_ki_safe).max(0);
            let ac_hit = survival * obs_autocall / S6;
            observation_autocall_first_hit[obs_idx] = ac_hit;

            if !is_maturity {
                redemption_pv += cfg.notional * ac_hit / S6;
                coupon_annuity += coupon_count * ac_hit;
                total_ac += ac_hit;
                total_ki += survival * obs_ki / S6;
                survival = survival * (S6 - obs_autocall) / S6;
            } else {
                let safe_principal = survival * obs_ki_safe / S6;
                let ki_redemption = survival * obs_ki_worst_ind / S6;
                redemption_pv += cfg.notional * safe_principal / S6;
                redemption_pv += cfg.notional * ki_redemption / S6;
                coupon_annuity += coupon_count * ac_hit;
                total_ki += survival * obs_ki / S6;
                total_ac += ac_hit;
            }
        }

        let loss = (cfg.notional - redemption_pv).max(0);
        let fair_coupon = if coupon_annuity > 100 {
            loss * S6 / coupon_annuity
        } else {
            0
        };
        C1FastTrace {
            quote: c1_fast_quote_from_components(
                cfg.notional,
                fair_coupon,
                redemption_pv,
                coupon_annuity,
                total_ki,
                total_ac,
            ),
            observation_survival,
            observation_autocall_first_hit,
        }
    }

    #[test]
    #[ignore]
    fn c1_fast_node_survival_anchor_snapshot() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        for sigma_common in [
            0.291_482_300_850_330_96,
            0.364_352_876_062_913_67,
            0.437_223_451_275_496_4,
        ] {
            let exact = model.quote_coupon(sigma_common).unwrap();
            let (cfg, drift_diffs, drift_shift_63) = c1_fast_inputs(sigma_common);
            let sigma_s6 = (sigma_common * S6 as f64).round() as i64;
            let scalar = quote_c1_fast_trace_scalar(&cfg, sigma_s6, drift_diffs, drift_shift_63);
            let node = quote_c1_fast_trace(&cfg, sigma_s6, drift_diffs, drift_shift_63);
            println!(
                "sigma={sigma_common:.15} exact_bps={:.6} scalar_bps={:.6} node_bps={:.6} node_err_bps={:.6} node_v0={:.9} node_u0={:.9} node_ki={:.9} node_ac={:.9} scalar_obs1_ac={:.9} node_obs1_ac={:.9} scalar_obs2_ac={:.9} node_obs2_ac={:.9} scalar_obs2_surv={:.9} node_obs2_surv={:.9}",
                exact.fair_coupon_bps,
                scalar.quote.fair_coupon_bps_f64(),
                node.quote.fair_coupon_bps_f64(),
                node.quote.fair_coupon_bps_f64() - exact.fair_coupon_bps,
                node.quote.zero_coupon_pv_f64(),
                node.quote.coupon_annuity_pv_f64(),
                node.quote.knock_in_rate_f64(),
                node.quote.autocall_rate_f64(),
                scalar.observation_autocall_first_hit[0] as f64 / S6 as f64,
                node.observation_autocall_first_hit[0] as f64 / S6 as f64,
                scalar.observation_autocall_first_hit[1] as f64 / S6 as f64,
                node.observation_autocall_first_hit[1] as f64 / S6 as f64,
                scalar.observation_survival[1] as f64 / S6 as f64,
                node.observation_survival[1] as f64 / S6 as f64,
            );
        }
    }
}
