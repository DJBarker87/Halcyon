//! Fast approximate triangle moments via dominant half-plane truncation.
//!
//! For c1 state propagation, the survivor conditional mean doesn't need
//! the full GL5 moment integral. The dominant half-plane approximation:
//! whichever barrier is closest to the distribution center determines
//! the conditional mean shift.
//!
//! E[u | w_k > a_k] ≈ μ_u + Cov(u, w_k) × φ(z_k) / [σ_wk × Φ(-z_k)]
//!
//! where w_k = a_u·u + a_v·v is the binding constraint projection.
//!
//! ~700 CU per call (3 norm_cdf_i64 + 3 norm_pdf_i64 + selection + arithmetic).

use crate::norm_cdf_fast::{norm_cdf_i64, norm_pdf_i64};
use crate::triangle_prob::TrianglePre64;

const S6: i64 = 1_000_000;
const SHIFT: i64 = 1_000_000;

/// Approximate triangle region moments via dominant half-plane.
///
/// Returns (probability, E[u], E[v]) at SCALE_6.
/// The probability uses the full inclusion-exclusion (exact).
/// The conditional means use the dominant half-plane approximation.
///
/// `mean_u`, `mean_v` at SCALE (i128). `rhs` at SCALE (i128).
/// `pre`: precomputed triangle geometry at SCALE_6.
/// `var_uu`, `cov_uv`, `var_vv` at SCALE (i128) — used for covariance projection.
#[inline(always)]
pub fn triangle_moment_dominant(
    mean_u: i128,
    mean_v: i128,
    rhs: [i128; 3],
    pre: &TrianglePre64,
    phi2_tables: [&[[i32; 64]; 64]; 3],
    var_uu: i128,
    cov_uv: i128,
    var_vv: i128,
) -> (i64, i64, i64) {
    use crate::bvn_cdf_phi2table::bvn_cdf_i64;

    let mu6 = (mean_u / SHIFT as i128) as i64;
    let mv6 = (mean_v / SHIFT as i128) as i64;

    // z_k for each half-plane (same as triangle_probability_i64)
    let mut z_s6 = [0i64; 3];
    let mut z_scale = [0i64; 3];
    for k in 0..3 {
        let rhs6 = (rhs[k] / SHIFT as i128) as i64;
        let ew6 = pre.au[k] * mu6 / S6 + pre.av[k] * mv6 / S6;
        let num6 = rhs6 - ew6;
        let z6 = num6 * pre.inv_std[k] / S6;
        z_s6[k] = z6;
        z_scale[k] = z6 * SHIFT;
    }

    // Probability via inclusion-exclusion (exact, same as triangle_probability_i64)
    let phi_z0 = norm_cdf_i64(z_scale[0]);
    let phi_z1 = norm_cdf_i64(z_scale[1]);
    let phi_z2 = norm_cdf_i64(z_scale[2]);
    let sum_complement = (S6 - phi_z0) + (S6 - phi_z1) + (S6 - phi_z2);

    let pairs: [(usize, usize); 3] = [(0, 1), (0, 2), (1, 2)];
    let mut sum_pair: i64 = 0;
    for (pidx, &(i, j)) in pairs.iter().enumerate() {
        let neg_zi = -z_scale[i];
        let neg_zj = -z_scale[j];
        let phi2 = if pre.phi2_neg[pidx] {
            let phi_a = norm_cdf_i64(neg_zi);
            let phi2_pos = bvn_cdf_i64(neg_zi, z_scale[j], phi2_tables[pidx]);
            (phi_a - phi2_pos).max(0)
        } else {
            bvn_cdf_i64(neg_zi, neg_zj, phi2_tables[pidx])
        };
        sum_pair += phi2;
    }

    let prob = (S6 - sum_complement + sum_pair).clamp(0, S6);

    if prob <= 0 {
        return (0, 0, 0);
    }

    // Dominant half-plane: find the binding constraint (smallest z_k = tightest barrier)
    // The constraint with smallest z_k has the highest exceedance probability Φ(-z_k).
    let dominant = if z_s6[0] <= z_s6[1] && z_s6[0] <= z_s6[2] {
        0
    } else if z_s6[1] <= z_s6[2] {
        1
    } else {
        2
    };

    // For the dominant half-plane w = au·u + av·v ≤ rhs:
    // The truncation ratio λ = φ(z) / Φ(z) (inverse Mills ratio at z_dom)
    let z_dom = z_scale[dominant];
    let phi_dom = norm_cdf_i64(z_dom);
    let pdf_dom = norm_pdf_i64(z_dom);

    // Avoid division by zero when Φ(z) is very small
    if phi_dom <= 100 {
        // Deep in the tail — conditional mean ≈ unconditional
        let eu = mu6 * prob / S6;
        let ev = mv6 * prob / S6;
        return (prob, eu, ev);
    }

    // λ = φ(z) / Φ(z) at S6
    let lambda = pdf_dom * S6 / phi_dom;

    // Conditional mean shift:
    // E[u | triangle] ≈ μ_u + Cov(u, w_dom) / Var(w_dom) × σ_w_dom × λ
    //                  = μ_u + (au·σ_uu + av·σ_uv) / σ_w × λ
    // At S6: shift_u = (au·var_uu + av·cov_uv) * inv_std / S6 × λ / S6
    let vuu6 = (var_uu / SHIFT as i128) as i64;
    let cuv6 = (cov_uv / SHIFT as i128) as i64;
    let vvv6 = (var_vv / SHIFT as i128) as i64;
    let au = pre.au[dominant];
    let av = pre.av[dominant];
    let inv_s = pre.inv_std[dominant];

    // cov_u_w = au * var_uu + av * cov_uv (at S6²/S6 = S6)
    let cov_u_w = au * vuu6 / S6 + av * cuv6 / S6;
    let cov_v_w = au * cuv6 / S6 + av * vvv6 / S6;

    // shift = cov_x_w * inv_std * lambda / S6² (at S6)
    let shift_u = cov_u_w * inv_s / S6 * lambda / S6;
    let shift_v = cov_v_w * inv_s / S6 * lambda / S6;

    // E[u] = (μ_u - shift_u) × P(triangle) — note: truncation removes the upper tail,
    // shifting the mean toward the barrier (subtract shift for "inside" region)
    let eu = (mu6 - shift_u) * prob / S6;
    let ev = (mv6 - shift_v) * prob / S6;

    (prob, eu, ev)
}
