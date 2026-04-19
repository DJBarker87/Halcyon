use crate::arithmetic::{fp_div_i, fp_mul_i, fp_sqrt};
use crate::bvn_cdf_phi2table::bvn_cdf_phi2table;
use crate::error::SolMathError;
use crate::norm_cdf_fast::{norm_cdf_fast, norm_cdf_i64, norm_pdf_i64};
use crate::normal::{norm_cdf_poly, norm_pdf};
use crate::overflow::checked_mul_div_i;
use crate::SCALE_I;

const GL5_NODES_LOCAL: [i128; 5] = [
    -906_179_845_939,
    -538_469_310_106,
    0,
    538_469_310_106,
    906_179_845_939,
];
const GL5_WEIGHTS_LOCAL: [i128; 5] = [
    236_926_885_056,
    478_628_670_499,
    568_888_888_889,
    478_628_670_499,
    236_926_885_056,
];
const GL7_NODES_LOCAL: [i128; 7] = [
    -949_107_912_343,
    -741_531_185_599,
    -405_845_151_377,
    0,
    405_845_151_377,
    741_531_185_599,
    949_107_912_343,
];
const GL7_WEIGHTS_LOCAL: [i128; 7] = [
    129_484_966_169,
    279_705_391_489,
    381_830_050_505,
    417_959_183_673,
    381_830_050_505,
    279_705_391_489,
    129_484_966_169,
];
const GL20_NODES_LOCAL: [i128; 20] = [
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
const GL20_WEIGHTS_LOCAL: [i128; 20] = [
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
const TRI_TOL: i128 = 1_000;

fn gl_rule(order: usize) -> Result<(&'static [i128], &'static [i128]), SolMathError> {
    match order {
        5 => Ok((&GL5_NODES_LOCAL, &GL5_WEIGHTS_LOCAL)),
        7 => Ok((&GL7_NODES_LOCAL, &GL7_WEIGHTS_LOCAL)),
        20 => Ok((&GL20_NODES_LOCAL, &GL20_WEIGHTS_LOCAL)),
        _ => Err(SolMathError::DomainError),
    }
}

/// One half-plane of the triangle: `a_u * u + a_v * v <= rhs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HalfPlane {
    pub a_u: i128,
    pub a_v: i128,
    pub rhs: i128,
}

/// Raw Gaussian moments over a triangle region.
///
/// All fields are scaled by `SCALE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TriangleRegionMoment {
    /// `P((U,V) in triangle)` at `SCALE`.
    pub probability: i128,
    /// `E[U * 1_{triangle}]` at `SCALE`.
    pub expectation_u: i128,
    /// `E[V * 1_{triangle}]` at `SCALE`.
    pub expectation_v: i128,
    /// `E[U^2 * 1_{triangle}]` at `SCALE`.
    pub expectation_uu: i128,
    /// `E[U V * 1_{triangle}]` at `SCALE`.
    pub expectation_uv: i128,
    /// `E[V^2 * 1_{triangle}]` at `SCALE`.
    pub expectation_vv: i128,
}

#[inline]
fn clamp_prob(value: i128) -> i128 {
    value.clamp(0, SCALE_I)
}

fn line_intersection(a: HalfPlane, b: HalfPlane) -> Result<Option<(i128, i128)>, SolMathError> {
    let det = a
        .a_u
        .checked_mul(b.a_v)
        .ok_or(SolMathError::Overflow)?
        .checked_sub(b.a_u.checked_mul(a.a_v).ok_or(SolMathError::Overflow)?)
        .ok_or(SolMathError::Overflow)?;
    if det == 0 {
        return Ok(None);
    }
    let num_u = a
        .rhs
        .checked_mul(b.a_v)
        .ok_or(SolMathError::Overflow)?
        .checked_sub(b.rhs.checked_mul(a.a_v).ok_or(SolMathError::Overflow)?)
        .ok_or(SolMathError::Overflow)?;
    let num_v = a
        .a_u
        .checked_mul(b.rhs)
        .ok_or(SolMathError::Overflow)?
        .checked_sub(b.a_u.checked_mul(a.rhs).ok_or(SolMathError::Overflow)?)
        .ok_or(SolMathError::Overflow)?;
    let u = checked_mul_div_i(num_u, SCALE_I, det)?;
    let v = checked_mul_div_i(num_v, SCALE_I, det)?;
    Ok(Some((u, v)))
}

fn satisfies(point: (i128, i128), planes: &[HalfPlane; 3]) -> Result<bool, SolMathError> {
    for plane in planes {
        let lhs = fp_mul_i(plane.a_u, point.0)?
            .checked_add(fp_mul_i(plane.a_v, point.1)?)
            .ok_or(SolMathError::Overflow)?;
        if lhs > plane.rhs + TRI_TOL {
            return Ok(false);
        }
    }
    Ok(true)
}

fn collect_vertices(planes: [HalfPlane; 3]) -> Result<[(i128, i128); 3], SolMathError> {
    let mut vertices = [(0i128, 0i128); 3];
    let mut count = 0usize;
    for i in 0..3 {
        for j in (i + 1)..3 {
            if let Some(point) = line_intersection(planes[i], planes[j])? {
                if satisfies(point, &planes)? {
                    let duplicate = vertices[..count].iter().any(|existing| {
                        (existing.0 - point.0).abs() <= TRI_TOL
                            && (existing.1 - point.1).abs() <= TRI_TOL
                    });
                    if !duplicate {
                        if count >= 3 {
                            return Err(SolMathError::DomainError);
                        }
                        vertices[count] = point;
                        count += 1;
                    }
                }
            }
        }
    }
    if count != 3 {
        return Err(SolMathError::DomainError);
    }
    vertices[..count].sort_by_key(|point| point.0);
    Ok(vertices)
}

fn vertical_section(
    vertices: &[(i128, i128); 3],
    u: i128,
) -> Result<Option<(i128, i128)>, SolMathError> {
    let mut hits = [0i128; 6];
    let mut hit_count = 0usize;
    for idx in 0..3 {
        let (x1, y1) = vertices[idx];
        let (x2, y2) = vertices[(idx + 1) % 3];
        let min_x = x1.min(x2);
        let max_x = x1.max(x2);
        if u < min_x - TRI_TOL || u > max_x + TRI_TOL {
            continue;
        }
        if x1 == x2 {
            hits[hit_count] = y1;
            hit_count += 1;
            hits[hit_count] = y2;
            hit_count += 1;
            continue;
        }
        let slope_num = y2 - y1;
        let slope_den = x2 - x1;
        let v = y1
            .checked_add(checked_mul_div_i(u - x1, slope_num, slope_den)?)
            .ok_or(SolMathError::Overflow)?;
        hits[hit_count] = v;
        hit_count += 1;
    }
    if hit_count < 2 {
        return Ok(None);
    }
    hits[..hit_count].sort();
    Ok(Some((hits[0], hits[hit_count - 1])))
}

/// Gaussian triangle probability for a 2D normal distribution.
///
/// Inputs:
/// - `mean_u`, `mean_v`: Gaussian means at `SCALE`.
/// - `var_uu`, `cov_uv`, `var_vv`: covariance entries at `SCALE`.
/// - `planes`: three half-planes whose intersection defines the triangle.
///
/// Returns:
/// - Probability at `SCALE`.
///
/// Error conditions:
/// - `DegenerateVariance` if the covariance is not positive definite.
/// - `DomainError` if the half-planes do not form a valid bounded triangle.
/// - arithmetic errors forwarded from the fixed-point helpers.
///
/// Accuracy:
/// - Empirically validated against a high-resolution `f64` reference across
///   160 generated cases with max absolute error below `2e-6`.
pub fn triangle_probability(
    mean_u: i128,
    mean_v: i128,
    var_uu: i128,
    cov_uv: i128,
    var_vv: i128,
    planes: [HalfPlane; 3],
) -> Result<i128, SolMathError> {
    triangle_probability_with_order(mean_u, mean_v, var_uu, cov_uv, var_vv, planes, 20)
}

/// Gaussian triangle probability using a selectable Gauss-Legendre order.
///
/// Supported `gl_order` values are `5`, `7`, and `20`.
///
/// Error conditions:
/// - `DegenerateVariance` if the covariance is not positive definite.
/// - `DomainError` if the half-planes do not form a valid bounded triangle or
///   if `gl_order` is unsupported.
/// - arithmetic errors forwarded from the fixed-point helpers.
pub fn triangle_probability_with_order(
    mean_u: i128,
    mean_v: i128,
    var_uu: i128,
    cov_uv: i128,
    var_vv: i128,
    planes: [HalfPlane; 3],
    gl_order: usize,
) -> Result<i128, SolMathError> {
    if var_uu <= 0 || var_vv <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let (gl_nodes, gl_weights) = gl_rule(gl_order)?;
    let cov_sq = fp_mul_i(cov_uv, cov_uv)?;
    let cond_var = var_vv
        .checked_sub(fp_div_i(cov_sq, var_uu)?)
        .ok_or(SolMathError::Overflow)?;
    if cond_var <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }

    let vertices = collect_vertices(planes)?;
    let sigma_u = fp_sqrt(var_uu as u128)? as i128;
    let sigma_v_cond = fp_sqrt(cond_var as u128)? as i128;
    let mut total = 0i128;

    for interval_idx in 0..2 {
        let left = vertices[interval_idx].0;
        let right = vertices[interval_idx + 1].0;
        if right - left <= TRI_TOL {
            continue;
        }
        let half = (right - left) / 2;
        let mid = (left + right) / 2;
        let mut weighted_sum = 0i128;
        for node_idx in 0..gl_nodes.len() {
            let u = mid + fp_mul_i(half, gl_nodes[node_idx])?;
            let Some((v_lo, v_hi)) = vertical_section(&vertices, u)? else {
                continue;
            };
            let z_u = fp_div_i(u - mean_u, sigma_u)?;
            let pdf_u = fp_div_i(norm_pdf(z_u)?, sigma_u)?;
            let cond_mean = mean_v
                .checked_add(fp_div_i(fp_mul_i(cov_uv, u - mean_u)?, var_uu)?)
                .ok_or(SolMathError::Overflow)?;
            let z_hi = fp_div_i(v_hi - cond_mean, sigma_v_cond)?;
            let z_lo = fp_div_i(v_lo - cond_mean, sigma_v_cond)?;
            let cdf_hi = norm_cdf_poly(z_hi)?;
            let cdf_lo = norm_cdf_poly(z_lo)?;
            let strip = if cdf_hi >= cdf_lo { cdf_hi - cdf_lo } else { 0 };
            let integrand = fp_mul_i(pdf_u, strip)?;
            let weighted = fp_mul_i(gl_weights[node_idx], integrand)?;
            weighted_sum = weighted_sum
                .checked_add(weighted)
                .ok_or(SolMathError::Overflow)?;
        }
        total = total
            .checked_add(fp_mul_i(half, weighted_sum)?)
            .ok_or(SolMathError::Overflow)?;
    }

    Ok(clamp_prob(total))
}

/// Precomputed triangle geometry — pure i64 fast path.
///
/// All fields at SCALE_6 (10⁶). Derived from frozen half-plane normals
/// and frozen residual covariance. Computed once, stored as const.
#[derive(Debug, Clone, Copy)]
pub struct TrianglePre64 {
    /// `a_u_k` at SCALE_6 for each half-plane.
    pub au: [i64; 3],
    /// `a_v_k` at SCALE_6 for each half-plane.
    pub av: [i64; 3],
    /// `1 / σ_wk` at SCALE_6 for each half-plane.
    pub inv_std: [i64; 3],
    /// Whether each pairwise projected correlation is negative.
    pub phi2_neg: [bool; 3],
}

/// Flattened triangle probability — pure i64, no Result, no function calls.
///
/// All CDF lookups inlined. Target: ~1.5K CU.
///
/// `mean_u`, `mean_v` at SCALE (1e12). `rhs` at SCALE (1e12).
/// Returns probability at SCALE (1e12).
#[inline(always)]
pub fn triangle_probability_i64(
    mean_u: i128,
    mean_v: i128,
    rhs: [i128; 3],
    pre: &TrianglePre64,
    phi2_tables: [&[[i32; 64]; 64]; 3],
) -> i128 {
    use crate::bvn_cdf_phi2table::bvn_cdf_i64;
    use crate::norm_cdf_fast::norm_cdf_i64;

    const S6: i64 = 1_000_000;
    const SHIFT: i64 = 1_000_000; // SCALE / S6

    // Narrow inputs to i64 at SCALE_6
    let mu6 = (mean_u / SHIFT as i128) as i64;
    let mv6 = (mean_v / SHIFT as i128) as i64;

    // z_k at SCALE (i64 × SHIFT for CDF lookups)
    let mut z_scale = [0i64; 3];
    for k in 0..3 {
        let rhs6 = (rhs[k] / SHIFT as i128) as i64;
        // ew6 = (au * mu + av * mv) / S6 — all i64
        let ew6 = (pre.au[k] as i64 * mu6 as i64 + pre.av[k] as i64 * mv6 as i64) / S6;
        let num6 = rhs6 - ew6;
        // z6 = num6 * inv_std / S6 — at SCALE_6
        let z6 = (num6 as i64 * pre.inv_std[k] as i64) / S6;
        // Scale to SCALE for CDF domain [-4e12, 4e12]
        z_scale[k] = z6 * SHIFT;
    }

    // Φ(−z_k) = S6 − Φ(z_k)  — all at SCALE_6
    let phi_z0 = norm_cdf_i64(z_scale[0]);
    let phi_z1 = norm_cdf_i64(z_scale[1]);
    let phi_z2 = norm_cdf_i64(z_scale[2]);
    let sum_complement = (S6 - phi_z0) + (S6 - phi_z1) + (S6 - phi_z2);

    // Φ₂(−z_i, −z_j; ρ) for pairs (0,1), (0,2), (1,2)
    let pairs: [(usize, usize); 3] = [(0, 1), (0, 2), (1, 2)];
    let mut sum_pair: i64 = 0;
    for (pidx, &(i, j)) in pairs.iter().enumerate() {
        let neg_zi = -z_scale[i];
        let neg_zj = -z_scale[j];
        let phi2 = if pre.phi2_neg[pidx] {
            // Φ₂(a,b;−ρ) = Φ(a) − Φ₂(a,−b;ρ)
            let phi_a = norm_cdf_i64(neg_zi);
            let phi2_pos = bvn_cdf_i64(neg_zi, z_scale[j], phi2_tables[pidx]);
            (phi_a - phi2_pos).max(0)
        } else {
            bvn_cdf_i64(neg_zi, neg_zj, phi2_tables[pidx])
        };
        sum_pair += phi2;
    }

    // P = 1 − Σ complement + Σ pair, clamp to [0, S6], upshift to SCALE
    let result_s6 = (S6 - sum_complement + sum_pair).clamp(0, S6);
    result_s6 as i128 * SHIFT as i128
}

/// Region moment output at SCALE_6.
#[derive(Debug, Clone, Copy, Default)]
pub struct RegionMoment6 {
    pub probability: i64,
    pub expectation_u: i64,
    pub expectation_v: i64,
    pub expectation_uu: i64,
    pub expectation_uv: i64,
    pub expectation_vv: i64,
}

/// i64 multiply at SCALE_6. Product fits i64 for |a|,|b| ≤ ~3e6.
#[inline(always)]
fn m6r(a: i64, b: i64) -> i64 {
    a * b / 1_000_000
}

/// Fast GL5 triangle region moments — fully i64, unrolled inner loop.
///
/// Vertices computed in i64. Vertical sections via precomputed edge slopes.
/// `norm_cdf_i64` and `norm_pdf_i64` for CDF/PDF. All multiplies direct i64.
/// ~7K CU target.
#[inline(always)]
pub fn triangle_region_moment_fast(
    mean_u: i128,
    mean_v: i128,
    var_uu: i128,
    cov_uv: i128,
    var_vv: i128,
    planes: [HalfPlane; 3],
) -> Result<RegionMoment6, SolMathError> {
    use crate::norm_cdf_fast::{norm_cdf_i64, norm_pdf_i64};

    const S6: i64 = 1_000_000;
    const SHIFT: i64 = 1_000_000;

    let mu = (mean_u / SHIFT as i128) as i64;
    let mv = (mean_v / SHIFT as i128) as i64;
    let vuu = (var_uu / SHIFT as i128) as i64;
    let cuv = (cov_uv / SHIFT as i128) as i64;
    let vvv = (var_vv / SHIFT as i128) as i64;

    if vuu <= 0 || vvv <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }

    // Cholesky — 2 sqrt via Newton (i64)
    let sigma_u = crate::i64_math::sqrt6(vuu)?;
    let cond_var = vvv - cuv * cuv / vuu;
    if cond_var <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let sigma_vc = crate::i64_math::sqrt6(cond_var)?;

    // Precomputed inverses (i64 divides, done once)
    let inv_su = S6 * S6 / sigma_u;
    let inv_svc = S6 * S6 / sigma_vc;
    let cuv_over_vuu = cuv * S6 / vuu;

    // Vertices in i64 at SCALE_6
    let verts = collect_vertices_i64(planes)?;

    // Precompute edge slopes for vertical section (avoid i128 per-node interpolation).
    // For each edge (v0→v1), slope = (y1-y0) * S6 / (x1-x0) — precomputed once.
    let mut edge_slope = [0i64; 3];
    let mut edge_x = [(0i64, 0i64); 3];
    let mut edge_y = [(0i64, 0i64); 3];
    for idx in 0..3 {
        let (x0, y0) = verts[idx];
        let (x1, y1) = verts[(idx + 1) % 3];
        edge_x[idx] = (x0, x1);
        edge_y[idx] = (y0, y1);
        let dx = x1 - x0;
        edge_slope[idx] = if dx.abs() > 0 { (y1 - y0) * S6 / dx } else { 0 };
    }

    // Inline vertical section using precomputed slopes.
    #[inline(always)]
    fn vert_section(
        u: i64,
        edge_x: &[(i64, i64); 3],
        edge_y: &[(i64, i64); 3],
        edge_slope: &[i64; 3],
    ) -> Option<(i64, i64)> {
        let mut lo = i64::MAX;
        let mut hi = i64::MIN;
        let mut hits = 0u8;
        for idx in 0..3 {
            let (x0, x1) = edge_x[idx];
            let mn = x0.min(x1);
            let mx = x0.max(x1);
            if u < mn || u > mx {
                continue;
            }
            let v = if x0 == x1 {
                (edge_y[idx].0 + edge_y[idx].1) / 2
            } else {
                edge_y[idx].0 + (u - x0) * edge_slope[idx] / 1_000_000
            };
            lo = lo.min(v);
            hi = hi.max(v);
            hits += 1;
        }
        if hits >= 2 {
            Some((lo, hi))
        } else {
            None
        }
    }

    let mut total = RegionMoment6::default();

    // GL5 over 2 intervals (left-to-mid, mid-to-right vertex)
    for interval in 0..2 {
        let left = verts[interval].0;
        let right = verts[interval + 1].0;
        if right - left <= 1 {
            continue;
        }
        let half = (right - left) / 2;
        let mid = (left + right) / 2;

        let mut wp = 0i64;
        let mut wu = 0i64;
        let mut wv = 0i64;
        let mut wuu = 0i64;
        let mut wuv = 0i64;
        let mut wvv = 0i64;

        // Unrolled GL5: 5 nodes
        macro_rules! gl_node {
            ($gl_n:expr, $gl_w:expr) => {{
                let u = mid + half * $gl_n / S6;
                let Some((v_lo, v_hi)) = vert_section(u, &edge_x, &edge_y, &edge_slope) else {
                    continue; // skip this node if no intersection
                };

                let z_u = (u - mu) * inv_su / S6;
                let phi_z = norm_pdf_i64(z_u * SHIFT);
                let pdf_u = phi_z * inv_su / S6;

                let cond_mean = mv + cuv_over_vuu * (u - mu) / S6;

                let z_hi = (v_hi - cond_mean) * inv_svc / S6;
                let z_lo = (v_lo - cond_mean) * inv_svc / S6;

                let cdf_hi = norm_cdf_i64(z_hi * SHIFT);
                let cdf_lo = norm_cdf_i64(z_lo * SHIFT);
                let prob_v = if cdf_hi >= cdf_lo { cdf_hi - cdf_lo } else { 0 };
                if prob_v == 0 {
                    continue;
                }

                let pdf_hi = norm_pdf_i64(z_hi * SHIFT);
                let pdf_lo = norm_pdf_i64(z_lo * SHIFT);
                let dpdf = pdf_lo - pdf_hi;

                let v_trunc = m6r(cond_mean, prob_v) + m6r(sigma_vc, dpdf);
                let sm_z = prob_v - m6r(z_hi, pdf_hi) + m6r(z_lo, pdf_lo);
                let cm_sq = m6r(cond_mean, cond_mean);
                let v_second = m6r(cm_sq, prob_v)
                    + 2 * m6r(m6r(cond_mean, sigma_vc), dpdf)
                    + m6r(cond_var, sm_z);

                let ip = m6r(pdf_u, prob_v);
                let gw: i64 = $gl_w;
                wp += m6r(gw, ip);
                wu += m6r(gw, m6r(ip, u));
                wv += m6r(gw, m6r(pdf_u, v_trunc));
                wuu += m6r(gw, m6r(ip, m6r(u, u)));
                wuv += m6r(gw, m6r(pdf_u, m6r(u, v_trunc)));
                wvv += m6r(gw, m6r(pdf_u, v_second));
            }};
        }

        // Can't use macro with continue across loop — use a loop with const index
        let gl_nodes: [i64; 5] = [-906_180, -538_469, 0, 538_469, 906_180];
        let gl_weights: [i64; 5] = [236_927, 478_629, 568_889, 478_629, 236_927];

        for ni in 0..5 {
            let u = mid + half * gl_nodes[ni] / S6;
            let Some((v_lo, v_hi)) = vert_section(u, &edge_x, &edge_y, &edge_slope) else {
                continue;
            };

            let z_u = (u - mu) * inv_su / S6;
            let phi_z = norm_pdf_i64(z_u * SHIFT);
            let pdf_u = phi_z * inv_su / S6;

            let cond_mean = mv + cuv_over_vuu * (u - mu) / S6;

            let z_hi = (v_hi - cond_mean) * inv_svc / S6;
            let z_lo = (v_lo - cond_mean) * inv_svc / S6;

            let cdf_hi = norm_cdf_i64(z_hi * SHIFT);
            let cdf_lo = norm_cdf_i64(z_lo * SHIFT);
            let prob_v = if cdf_hi >= cdf_lo { cdf_hi - cdf_lo } else { 0 };
            if prob_v == 0 {
                continue;
            }

            let pdf_hi = norm_pdf_i64(z_hi * SHIFT);
            let pdf_lo = norm_pdf_i64(z_lo * SHIFT);
            let dpdf = pdf_lo - pdf_hi;

            let v_trunc = m6r(cond_mean, prob_v) + m6r(sigma_vc, dpdf);
            let sm_z = prob_v - m6r(z_hi, pdf_hi) + m6r(z_lo, pdf_lo);
            let cm_sq = m6r(cond_mean, cond_mean);
            let v_second =
                m6r(cm_sq, prob_v) + 2 * m6r(m6r(cond_mean, sigma_vc), dpdf) + m6r(cond_var, sm_z);

            let ip = m6r(pdf_u, prob_v);
            let gw = gl_weights[ni];
            wp += m6r(gw, ip);
            wu += m6r(gw, m6r(ip, u));
            wv += m6r(gw, m6r(pdf_u, v_trunc));
            wuu += m6r(gw, m6r(ip, m6r(u, u)));
            wuv += m6r(gw, m6r(pdf_u, m6r(u, v_trunc)));
            wvv += m6r(gw, m6r(pdf_u, v_second));
        }

        total.probability += m6r(half, wp);
        total.expectation_u += m6r(half, wu);
        total.expectation_v += m6r(half, wv);
        total.expectation_uu += m6r(half, wuu);
        total.expectation_uv += m6r(half, wuv);
        total.expectation_vv += m6r(half, wvv);
    }

    total.probability = total.probability.clamp(0, S6);
    Ok(total)
}

/// GL5 nodes at SCALE_6 for the moment quadrature.
const GL5_NODES_6: [i64; 5] = [-906_180, -538_469, 0, 538_469, 906_180];
/// GL5 weights at SCALE_6 (sum = 2·S6).
const GL5_WEIGHTS_6: [i64; 5] = [236_927, 478_629, 568_889, 478_629, 236_927];

/// Triangle region moments via GL5 quadrature in pure i64.
///
/// Same semantics as `triangle_region_moment_with_order` but all arithmetic
/// in i64 at SCALE_6 using `norm_cdf_i64` and `norm_pdf_i64` table lookups.
///
/// Inputs at SCALE (i128). Vertices from the frozen half-planes.
/// Returns `RegionMoment6` at SCALE_6.
#[inline(always)]
pub fn triangle_region_moment_i64(
    mean_u: i128,
    mean_v: i128,
    var_uu: i128,
    cov_uv: i128,
    var_vv: i128,
    planes: [HalfPlane; 3],
) -> Result<RegionMoment6, SolMathError> {
    // Narrow to i64 at SCALE_6
    const S6: i64 = 1_000_000;
    const SHIFT: i64 = 1_000_000;

    let mu = (mean_u / SHIFT as i128) as i64;
    let mv = (mean_v / SHIFT as i128) as i64;
    let vuu = (var_uu / SHIFT as i128) as i64;
    let cuv = (cov_uv / SHIFT as i128) as i64;
    let vvv = (var_vv / SHIFT as i128) as i64;

    if vuu <= 0 || vvv <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }

    // Cholesky for conditional v|u
    let sigma_u = crate::i64_math::sqrt6(vuu)?;
    let cond_var = vvv - cuv * cuv / vuu;
    if cond_var <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let sigma_v_cond = crate::i64_math::sqrt6(cond_var)?;

    // Precomputed inverses (3 i64 divides, done once)
    let inv_sigma_u = S6 * S6 / sigma_u;
    let inv_sigma_vc = S6 * S6 / sigma_v_cond;
    let cuv_over_vuu = cuv * S6 / vuu;

    // Collect triangle vertices in i64 at SCALE_6.
    let verts6 = collect_vertices_i64(planes)?;

    let mut total = RegionMoment6::default();

    for interval_idx in 0..2 {
        let left = verts6[interval_idx].0;
        let right = verts6[interval_idx + 1].0;
        if right - left <= 1 {
            continue;
        }
        let half = (right - left) / 2;
        let mid_u = (left + right) / 2;

        let mut wp = 0i64;
        let mut wu = 0i64;
        let mut wv = 0i64;
        let mut wuu = 0i64;
        let mut wuv = 0i64;
        let mut wvv = 0i64;

        for ni in 0..5 {
            let u = mid_u + m6r(half, GL5_NODES_6[ni]);
            let Some((v_lo, v_hi)) = vertical_section_i64(&verts6, u) else {
                continue;
            };

            let z_u_s6 = m6r(u - mu, inv_sigma_u);
            let phi_z = norm_pdf_i64(z_u_s6 * SHIFT);
            let pdf_u = m6r(phi_z, inv_sigma_u); // φ(z)/σ at S6

            let cond_mean = mv + m6r(cuv_over_vuu, u - mu);

            let z_hi = m6r(v_hi - cond_mean, inv_sigma_vc);
            let z_lo = m6r(v_lo - cond_mean, inv_sigma_vc);

            let cdf_hi = norm_cdf_i64(z_hi * SHIFT);
            let cdf_lo = norm_cdf_i64(z_lo * SHIFT);
            let prob_v = if cdf_hi >= cdf_lo { cdf_hi - cdf_lo } else { 0 };
            if prob_v == 0 {
                continue;
            }

            let pdf_hi = norm_pdf_i64(z_hi * SHIFT);
            let pdf_lo = norm_pdf_i64(z_lo * SHIFT);
            let dpdf = pdf_lo - pdf_hi;

            let v_trunc = m6r(cond_mean, prob_v) + m6r(sigma_v_cond, dpdf);

            let sm_z = prob_v - m6r(z_hi, pdf_hi) + m6r(z_lo, pdf_lo);
            let cm_sq = m6r(cond_mean, cond_mean);
            let v_second = m6r(cm_sq, prob_v)
                + 2 * m6r(m6r(cond_mean, sigma_v_cond), dpdf)
                + m6r(cond_var, sm_z);

            let ip = m6r(pdf_u, prob_v);
            let iu = m6r(ip, u);
            let iv = m6r(pdf_u, v_trunc);
            let u_sq = m6r(u, u);
            let iuu = m6r(ip, u_sq);
            let uv_prod = m6r(u, v_trunc);
            let iuv = m6r(pdf_u, uv_prod);
            let ivv = m6r(pdf_u, v_second);

            let gw = GL5_WEIGHTS_6[ni];
            wp += m6r(gw, ip);
            wu += m6r(gw, iu);
            wv += m6r(gw, iv);
            wuu += m6r(gw, iuu);
            wuv += m6r(gw, iuv);
            wvv += m6r(gw, ivv);
        }

        total.probability += m6r(half, wp);
        total.expectation_u += m6r(half, wu);
        total.expectation_v += m6r(half, wv);
        total.expectation_uu += m6r(half, wuu);
        total.expectation_uv += m6r(half, wuv);
        total.expectation_vv += m6r(half, wvv);
    }

    total.probability = total.probability.clamp(0, S6);
    Ok(total)
}

/// Collect triangle vertices in i64 at SCALE_6. Uses i128 only for the 2×2 solve.
#[inline(always)]
fn collect_vertices_i64(planes: [HalfPlane; 3]) -> Result<[(i64, i64); 3], SolMathError> {
    const SHIFT: i64 = 1_000_000;
    let mut verts = [(0i64, 0i64); 3];
    let mut count = 0usize;
    for i in 0..3 {
        for j in (i + 1)..3 {
            let au = (planes[i].a_u / SHIFT as i128) as i64;
            let av = (planes[i].a_v / SHIFT as i128) as i64;
            let ar = (planes[i].rhs / SHIFT as i128) as i64;
            let bu = (planes[j].a_u / SHIFT as i128) as i64;
            let bv = (planes[j].a_v / SHIFT as i128) as i64;
            let br = (planes[j].rhs / SHIFT as i128) as i64;
            // det, num_u, num_v use i128 for the cross products (max ~1e12)
            let det = au as i128 * bv as i128 - bu as i128 * av as i128;
            if det == 0 {
                continue;
            }
            let num_u = ar as i128 * bv as i128 - br as i128 * av as i128;
            let num_v = au as i128 * br as i128 - bu as i128 * ar as i128;
            let u = (num_u * 1_000_000 / det) as i64;
            let v = (num_v * 1_000_000 / det) as i64;
            if count < 3 {
                verts[count] = (u, v);
                count += 1;
            }
        }
    }
    if count != 3 {
        return Err(SolMathError::DomainError);
    }
    // Sort by u
    if verts[0].0 > verts[1].0 {
        verts.swap(0, 1);
    }
    if verts[1].0 > verts[2].0 {
        verts.swap(1, 2);
    }
    if verts[0].0 > verts[1].0 {
        verts.swap(0, 1);
    }
    Ok(verts)
}

/// i64 vertical section helper.
#[inline(always)]
fn vertical_section_i64(verts: &[(i64, i64); 3], u: i64) -> Option<(i64, i64)> {
    let mut lo = i64::MAX;
    let mut hi = i64::MIN;
    let mut hits = 0u8;
    for idx in 0..3 {
        let (x1, y1) = verts[idx];
        let (x2, y2) = verts[(idx + 1) % 3];
        if (u < x1.min(x2)) || (u > x1.max(x2)) {
            continue;
        }
        let v = if x1 == x2 {
            (y1 + y2) / 2
        } else {
            y1 + ((u - x1) as i128 * (y2 - y1) as i128 / (x2 - x1) as i128) as i64
        };
        lo = lo.min(v);
        hi = hi.max(v);
        hits += 1;
    }
    if hits >= 2 {
        Some((lo, hi))
    } else {
        None
    }
}

/// Fused triangle probability + moments in one pass. Pure i64.
///
/// Returns `(probability, E[u], E[v], E[u²], E[uv], E[v²])` at SCALE_6.
/// The probability is exact (inclusion-exclusion). The moments use
/// first-order truncated-normal corrections from each half-plane,
/// adding ~1K CU on top of the 3.2K probability computation.
///
/// Inputs: `mean_u`, `mean_v`, `rhs` at SCALE (i128).
/// `pre`: precomputed triangle geometry.
/// `cov_proj`: precomputed `[Cov(u,w_k)/σ_wk, Cov(v,w_k)/σ_wk]` for k=0,1,2 at SCALE_6.
///   These are frozen: derived from loadings × residual covariance × inv_std.
#[inline(always)]
pub fn triangle_probability_and_moments_i64(
    mean_u: i128,
    mean_v: i128,
    rhs: [i128; 3],
    pre: &TrianglePre64,
    phi2_tables: [&[[i32; 64]; 64]; 3],
    cov_proj: &[[i64; 2]; 3], // [Cov(u,w_k)/σ_wk, Cov(v,w_k)/σ_wk] per plane, at S6
    pair_rho: &[i64; 3],      // signed projected correlation for pairs (0,1), (0,2), (1,2), at S6
    pair_inv_sqrt_1mrho2: &[i64; 3], // 1/sqrt(1-rho^2) for the same pairs, at S6
) -> RegionMoment6 {
    use crate::bvn_cdf_phi2table::bvn_cdf_i64;
    use crate::norm_cdf_fast::{norm_cdf_i64, norm_pdf_i64};

    const S6: i64 = 1_000_000;
    const S6_I128: i128 = 1_000_000;
    const S12_I128: i128 = 1_000_000_000_000;
    const SHIFT: i64 = 1_000_000;

    let mu6 = (mean_u / SHIFT as i128) as i64;
    let mv6 = (mean_v / SHIFT as i128) as i64;

    // z_k computation (same as triangle_probability_i64)
    let mut z_s6 = [0i64; 3];
    let mut z_scale = [0i64; 3];
    let mut phi_z = [0i64; 3];
    let mut pdf_z = [0i64; 3];

    for k in 0..3 {
        let rhs6 = (rhs[k] / SHIFT as i128) as i64;
        let ew6 = (pre.au[k] * mu6 + pre.av[k] * mv6) / S6;
        let num6 = rhs6 - ew6;
        z_s6[k] = num6 * pre.inv_std[k] / S6;
        z_scale[k] = z_s6[k] * SHIFT;
        phi_z[k] = norm_cdf_i64(z_scale[k]);
        pdf_z[k] = norm_pdf_i64(z_scale[k]);
    }

    // Probability via inclusion-exclusion
    let sum_complement = (S6 - phi_z[0]) + (S6 - phi_z[1]) + (S6 - phi_z[2]);

    let pairs: [(usize, usize); 3] = [(0, 1), (0, 2), (1, 2)];
    let mut sum_pair: i64 = 0;
    let mut pair_u_shift_num: i128 = 0;
    let mut pair_v_shift_num: i128 = 0;
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

        // Exact first raw moments for the pairwise complement come from
        // differentiating Φ₂ with respect to the standardized cut levels.
        // For A_i = {w_i > rhs_i}, A_j = {w_j > rhs_j}:
        //   E[x 1_{A_i∩A_j}] = μ_x P_ij
        //                    + Cov(x,w_i)/σ_i * ∂Φ₂/∂(-z_i)
        //                    + Cov(x,w_j)/σ_j * ∂Φ₂/∂(-z_j)
        // with
        //   ∂Φ₂(a,b;ρ)/∂a = φ(a) Φ((b - ρ a)/sqrt(1-ρ²)).
        let rho = pair_rho[pidx];
        let inv_sqrt = pair_inv_sqrt_1mrho2[pidx];
        let cond_i = ((((rho as i128 * z_s6[i] as i128) - (z_s6[j] as i128 * S6_I128))
            * inv_sqrt as i128)
            / S12_I128) as i64;
        let cond_j = ((((rho as i128 * z_s6[j] as i128) - (z_s6[i] as i128 * S6_I128))
            * inv_sqrt as i128)
            / S12_I128) as i64;
        let deriv_i_num = pdf_z[i] as i128 * norm_cdf_i64(cond_i * SHIFT) as i128;
        let deriv_j_num = pdf_z[j] as i128 * norm_cdf_i64(cond_j * SHIFT) as i128;
        pair_u_shift_num +=
            cov_proj[i][0] as i128 * deriv_i_num + cov_proj[j][0] as i128 * deriv_j_num;
        pair_v_shift_num +=
            cov_proj[i][1] as i128 * deriv_i_num + cov_proj[j][1] as i128 * deriv_j_num;
    }

    let prob = (S6 - sum_complement + sum_pair).clamp(0, S6);
    if prob <= 0 {
        return RegionMoment6::default();
    }

    // Exact first raw moments via inclusion-exclusion:
    //   E[x 1_T] = μ_x P(T)
    //            - Σ_k Cov(x,w_k)/σ_k φ(z_k)
    //            + Σ_{i<j} pair_shift_{ij}(x)
    //
    // where pair_shift_{ij}(x) uses the Φ₂ derivative terms computed above.
    let mut single_u_shift_num: i128 = 0;
    let mut single_v_shift_num: i128 = 0;
    for k in 0..3 {
        single_u_shift_num += cov_proj[k][0] as i128 * pdf_z[k] as i128;
        single_v_shift_num += cov_proj[k][1] as i128 * pdf_z[k] as i128;
    }

    // Raw first moments are exact for the triangle under the frozen geometry.
    let eu = ((((mu6 as i128 * prob as i128) - single_u_shift_num) * S6_I128) + pair_u_shift_num)
        / S12_I128;
    let ev = ((((mv6 as i128 * prob as i128) - single_v_shift_num) * S6_I128) + pair_v_shift_num)
        / S12_I128;

    // Second moments stay on the cheapest unconditional scaling in this low-CU path.
    let eu2 = mu6 * mu6 / S6 * prob / S6;
    let euv = mu6 * mv6 / S6 * prob / S6;
    let ev2 = mv6 * mv6 / S6 * prob / S6;

    RegionMoment6 {
        probability: prob,
        expectation_u: eu as i64,
        expectation_v: ev as i64,
        expectation_uu: eu2,
        expectation_uv: euv,
        expectation_vv: ev2,
    }
}

/// Precomputed triangle geometry for the on-chain fast path.
///
/// All fields derived from frozen half-plane normals `(a_u, a_v)` and
/// frozen residual covariance.  Computed once at deployment, stored as
/// const — no `fp_sqrt` or `fp_div` on-chain.
#[derive(Debug, Clone, Copy)]
pub struct TrianglePrecomputed {
    /// `1 / σ_wk` for each of the 3 half-plane projections, at SCALE.
    pub inv_std: [i128; 3],
    /// Loading projection `c_k = [a_u_k, a_v_k]` dot product weights.
    /// `E[w_k] = au_k · μ_u + av_k · μ_v`, so we store `(au_k, av_k)`.
    pub au: [i128; 3],
    pub av: [i128; 3],
    /// Whether each pairwise projected correlation is negative.
    /// Pairs: `(0,1), (0,2), (1,2)`.
    pub phi2_neg: [bool; 3],
}

/// Triangle probability with precomputed geometry — the on-chain hot path.
///
/// Per call: 6 multiplies + 3 `norm_cdf_poly` + 3 `bvn_cdf_phi2table` +
/// adds.  No `fp_sqrt`, no `fp_div`, no variance projection.
///
/// `z_k = (rhs_k − a_u_k · μ_u − a_v_k · μ_v) × inv_std_k`
pub fn triangle_probability_precomputed(
    mean_u: i128,
    mean_v: i128,
    rhs: [i128; 3],
    pre: &TrianglePrecomputed,
    phi2_tables: [&[[i32; 64]; 64]; 3],
) -> Result<i128, SolMathError> {
    // z_k = (rhs_k - au_k*mean_u - av_k*mean_v) * inv_std_k
    // All at SCALE (1e12). The multiply au*mean is i128 but we only need
    // the result at SCALE, so we do: (au * mean) / SCALE via fp_mul_i.
    // To avoid 3× i128 fp_mul_i (~2K each), narrow to i64 at SCALE_6:
    //   au6 = au / 1e6, mean6 = mean / 1e6 → product / 1e6 = at SCALE_6
    //   Then upshift result back to SCALE for the CDF lookups.
    const S6: i64 = 1_000_000;
    const SHIFT: i64 = 1_000_000;
    let mu6 = (mean_u / S6 as i128) as i64;
    let mv6 = (mean_v / S6 as i128) as i64;
    let mut z = [0i128; 3];
    for k in 0..3 {
        let au6 = (pre.au[k] / S6 as i128) as i64;
        let av6 = (pre.av[k] / S6 as i128) as i64;
        let rhs6 = (rhs[k] / S6 as i128) as i64;
        let inv6 = (pre.inv_std[k] / S6 as i128) as i64;
        // ew6 = (au6 * mu6 + av6 * mv6) / S6  — at SCALE_6
        let ew6 = (au6 as i64 * mu6 as i64 + av6 as i64 * mv6 as i64) / S6;
        let num6 = rhs6 - ew6;
        // z6 = num6 * inv6 / S6 — at SCALE_6
        let z6 = (num6 as i64 * inv6 as i64) / S6;
        // Upshift to SCALE for CDF lookups
        z[k] = z6 as i128 * SHIFT as i128;
    }

    // P(triangle) = 1 − Σ Φ(−z_k) + Σ Φ₂(−z_i, −z_j; ρ_ij)
    // All Φ via norm_cdf_fast (i64 table, ~200 CU) not norm_cdf_poly (i128, ~6.4K CU).
    let mut sum_complement = 0i128;
    for k in 0..3 {
        sum_complement = sum_complement
            .checked_add(
                SCALE_I
                    .checked_sub(norm_cdf_fast(z[k])?)
                    .ok_or(SolMathError::Overflow)?,
            )
            .ok_or(SolMathError::Overflow)?;
    }

    const PAIRS: [(usize, usize); 3] = [(0, 1), (0, 2), (1, 2)];
    let mut sum_pair = 0i128;
    for (pidx, &(i, j)) in PAIRS.iter().enumerate() {
        let neg_zi = z[i].checked_neg().ok_or(SolMathError::Overflow)?;
        let neg_zj = z[j].checked_neg().ok_or(SolMathError::Overflow)?;
        let phi2 = if pre.phi2_neg[pidx] {
            // Φ₂(a,b;-ρ) = Φ(a) - Φ₂(a,-b;ρ)
            let phi_a = norm_cdf_fast(neg_zi)?;
            phi_a
                .checked_sub(bvn_cdf_phi2table(neg_zi, z[j], phi2_tables[pidx])?)
                .ok_or(SolMathError::Overflow)?
        } else {
            bvn_cdf_phi2table(neg_zi, neg_zj, phi2_tables[pidx])?
        };
        sum_pair = sum_pair
            .checked_add(phi2.max(0))
            .ok_or(SolMathError::Overflow)?;
    }

    let result = SCALE_I
        .checked_sub(sum_complement)
        .ok_or(SolMathError::Overflow)?
        .checked_add(sum_pair)
        .ok_or(SolMathError::Overflow)?;
    Ok(clamp_prob(result))
}

/// Triangle probability via inclusion-exclusion with precomputed Φ₂ tables.
///
/// Replaces the GL-quadrature inner loop with 3 Φ + 3 Φ₂ table lookups.
/// The triple-complement term is zero when the 3 half-planes form a bounded
/// triangle (rank-2 covariance ⟹ outward normals span all directions).
///
/// `phi2_tables[k]` is the Φ₂ table (64×64, SCALE_6) for pair k of the
/// ordered pairs `(0,1), (0,2), (1,2)`.  Tables must be generated at
/// `|ρ_ij|`; set `phi2_neg[k] = true` when the projected correlation for
/// that pair is negative so the identity `Φ₂(a,b;-ρ) = Φ(a) − Φ₂(a,−b;ρ)`
/// is applied automatically.
pub fn triangle_probability_phi2(
    mean_u: i128,
    mean_v: i128,
    var_uu: i128,
    cov_uv: i128,
    var_vv: i128,
    planes: [HalfPlane; 3],
    phi2_tables: [&[[i32; 64]; 64]; 3],
    phi2_neg: [bool; 3],
) -> Result<i128, SolMathError> {
    if var_uu <= 0 || var_vv <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }

    // For each half-plane k:  w_k = a_u · u + a_v · v
    //   E[w_k]   = a_u · μ_u + a_v · μ_v
    //   Var(w_k) = a_u² σ_uu + 2 a_u a_v σ_uv + a_v² σ_vv
    //   z_k      = (rhs_k − E[w_k]) / √Var(w_k)
    let mut z = [0i128; 3];
    for k in 0..3 {
        let au = planes[k].a_u;
        let av = planes[k].a_v;
        let ew = fp_mul_i(au, mean_u)?
            .checked_add(fp_mul_i(av, mean_v)?)
            .ok_or(SolMathError::Overflow)?;
        let vw = fp_mul_i(fp_mul_i(au, au)?, var_uu)?
            .checked_add(
                2_i128
                    .checked_mul(fp_mul_i(fp_mul_i(au, av)?, cov_uv)?)
                    .ok_or(SolMathError::Overflow)?,
            )
            .ok_or(SolMathError::Overflow)?
            .checked_add(fp_mul_i(fp_mul_i(av, av)?, var_vv)?)
            .ok_or(SolMathError::Overflow)?;
        if vw <= 0 {
            return Err(SolMathError::DegenerateVariance);
        }
        let std_w = fp_sqrt(vw as u128)? as i128;
        z[k] = fp_div_i(
            planes[k]
                .rhs
                .checked_sub(ew)
                .ok_or(SolMathError::Overflow)?,
            std_w,
        )?;
    }

    // P(triangle) = 1 − Σ Φ(−z_k) + Σ Φ₂(−z_i, −z_j; ρ_ij)

    let mut sum_complement = 0i128;
    for k in 0..3 {
        let phi_neg = SCALE_I
            .checked_sub(norm_cdf_poly(z[k])?)
            .ok_or(SolMathError::Overflow)?;
        sum_complement = sum_complement
            .checked_add(phi_neg)
            .ok_or(SolMathError::Overflow)?;
    }

    const PAIRS: [(usize, usize); 3] = [(0, 1), (0, 2), (1, 2)];
    let mut sum_pair = 0i128;
    for (pidx, &(i, j)) in PAIRS.iter().enumerate() {
        let neg_zi = z[i].checked_neg().ok_or(SolMathError::Overflow)?;
        let neg_zj = z[j].checked_neg().ok_or(SolMathError::Overflow)?;
        let phi2 = if phi2_neg[pidx] {
            // ρ < 0 ⟹ Φ₂(a, b; −|ρ|) = Φ(a) − Φ₂(a, −b; |ρ|)
            let phi_a = norm_cdf_poly(neg_zi)?;
            let pos_zj = z[j]; // −(−z_j) = z_j
            phi_a
                .checked_sub(bvn_cdf_phi2table(neg_zi, pos_zj, phi2_tables[pidx])?)
                .ok_or(SolMathError::Overflow)?
        } else {
            bvn_cdf_phi2table(neg_zi, neg_zj, phi2_tables[pidx])?
        };
        sum_pair = sum_pair
            .checked_add(phi2.max(0))
            .ok_or(SolMathError::Overflow)?;
    }

    let result = SCALE_I
        .checked_sub(sum_complement)
        .ok_or(SolMathError::Overflow)?
        .checked_add(sum_pair)
        .ok_or(SolMathError::Overflow)?;
    Ok(clamp_prob(result))
}

/// Raw Gaussian moments over a triangle region.
///
/// Inputs:
/// - `mean_u`, `mean_v`: Gaussian means at `SCALE`.
/// - `var_uu`, `cov_uv`, `var_vv`: covariance entries at `SCALE`.
/// - `planes`: three half-planes whose intersection defines the triangle.
///
/// Returns:
/// - raw probability / first / second moments at `SCALE`.
///
/// Error conditions:
/// - `DegenerateVariance` if the covariance is not positive definite.
/// - `DomainError` if the half-planes do not form a valid bounded triangle.
/// - arithmetic errors forwarded from the fixed-point helpers.
pub fn triangle_region_moment(
    mean_u: i128,
    mean_v: i128,
    var_uu: i128,
    cov_uv: i128,
    var_vv: i128,
    planes: [HalfPlane; 3],
) -> Result<TriangleRegionMoment, SolMathError> {
    triangle_region_moment_with_order(mean_u, mean_v, var_uu, cov_uv, var_vv, planes, 20)
}

/// Raw Gaussian moments over a triangle region using a selectable
/// Gauss-Legendre order.
///
/// Supported `gl_order` values are `5`, `7`, and `20`.
///
/// Error conditions:
/// - `DegenerateVariance` if the covariance is not positive definite.
/// - `DomainError` if the half-planes do not form a valid bounded triangle or
///   if `gl_order` is unsupported.
/// - arithmetic errors forwarded from the fixed-point helpers.
pub fn triangle_region_moment_with_order(
    mean_u: i128,
    mean_v: i128,
    var_uu: i128,
    cov_uv: i128,
    var_vv: i128,
    planes: [HalfPlane; 3],
    gl_order: usize,
) -> Result<TriangleRegionMoment, SolMathError> {
    if var_uu <= 0 || var_vv <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let (gl_nodes, gl_weights) = gl_rule(gl_order)?;
    let cov_sq = fp_mul_i(cov_uv, cov_uv)?;
    let cond_var = var_vv
        .checked_sub(fp_div_i(cov_sq, var_uu)?)
        .ok_or(SolMathError::Overflow)?;
    if cond_var <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }

    let vertices = collect_vertices(planes)?;
    let sigma_u = fp_sqrt(var_uu as u128)? as i128;
    let sigma_v_cond = fp_sqrt(cond_var as u128)? as i128;
    let mut total = TriangleRegionMoment::default();

    for interval_idx in 0..2 {
        let left = vertices[interval_idx].0;
        let right = vertices[interval_idx + 1].0;
        if right - left <= TRI_TOL {
            continue;
        }
        let half = (right - left) / 2;
        let mid = (left + right) / 2;
        let mut weighted_probability = 0i128;
        let mut weighted_u = 0i128;
        let mut weighted_v = 0i128;
        let mut weighted_uu = 0i128;
        let mut weighted_uv = 0i128;
        let mut weighted_vv = 0i128;

        for node_idx in 0..gl_nodes.len() {
            let u = mid + fp_mul_i(half, gl_nodes[node_idx])?;
            let Some((v_lo, v_hi)) = vertical_section(&vertices, u)? else {
                continue;
            };
            let z_u = fp_div_i(u - mean_u, sigma_u)?;
            let pdf_u = fp_div_i(norm_pdf(z_u)?, sigma_u)?;
            let cond_mean = mean_v
                .checked_add(fp_div_i(fp_mul_i(cov_uv, u - mean_u)?, var_uu)?)
                .ok_or(SolMathError::Overflow)?;
            let z_hi = fp_div_i(v_hi - cond_mean, sigma_v_cond)?;
            let z_lo = fp_div_i(v_lo - cond_mean, sigma_v_cond)?;
            let cdf_hi = norm_cdf_poly(z_hi)?;
            let cdf_lo = norm_cdf_poly(z_lo)?;
            let prob_v = if cdf_hi >= cdf_lo { cdf_hi - cdf_lo } else { 0 };
            if prob_v == 0 {
                continue;
            }
            let pdf_hi = norm_pdf(z_hi)?;
            let pdf_lo = norm_pdf(z_lo)?;
            let v_truncated = fp_mul_i(cond_mean, prob_v)?
                .checked_add(fp_mul_i(
                    sigma_v_cond,
                    pdf_lo.checked_sub(pdf_hi).ok_or(SolMathError::Overflow)?,
                )?)
                .ok_or(SolMathError::Overflow)?;

            let second_moment_z = prob_v
                .checked_sub(fp_mul_i(z_hi, pdf_hi)?)
                .ok_or(SolMathError::Overflow)?
                .checked_add(fp_mul_i(z_lo, pdf_lo)?)
                .ok_or(SolMathError::Overflow)?;
            let cond_mean_sq = fp_mul_i(cond_mean, cond_mean)?;
            let v_second = fp_mul_i(cond_mean_sq, prob_v)?
                .checked_add(
                    2_i128
                        .checked_mul(fp_mul_i(
                            fp_mul_i(cond_mean, sigma_v_cond)?,
                            pdf_lo.checked_sub(pdf_hi).ok_or(SolMathError::Overflow)?,
                        )?)
                        .ok_or(SolMathError::Overflow)?,
                )
                .ok_or(SolMathError::Overflow)?
                .checked_add(fp_mul_i(cond_var, second_moment_z)?)
                .ok_or(SolMathError::Overflow)?;

            let integrand_probability = fp_mul_i(pdf_u, prob_v)?;
            let u_sq = fp_mul_i(u, u)?;
            let uv = fp_mul_i(u, v_truncated)?;
            let integrand_u = fp_mul_i(integrand_probability, u)?;
            let integrand_v = fp_mul_i(pdf_u, v_truncated)?;
            let integrand_uu = fp_mul_i(integrand_probability, u_sq)?;
            let integrand_uv = fp_mul_i(pdf_u, uv)?;
            let integrand_vv = fp_mul_i(pdf_u, v_second)?;

            weighted_probability = weighted_probability
                .checked_add(fp_mul_i(gl_weights[node_idx], integrand_probability)?)
                .ok_or(SolMathError::Overflow)?;
            weighted_u = weighted_u
                .checked_add(fp_mul_i(gl_weights[node_idx], integrand_u)?)
                .ok_or(SolMathError::Overflow)?;
            weighted_v = weighted_v
                .checked_add(fp_mul_i(gl_weights[node_idx], integrand_v)?)
                .ok_or(SolMathError::Overflow)?;
            weighted_uu = weighted_uu
                .checked_add(fp_mul_i(gl_weights[node_idx], integrand_uu)?)
                .ok_or(SolMathError::Overflow)?;
            weighted_uv = weighted_uv
                .checked_add(fp_mul_i(gl_weights[node_idx], integrand_uv)?)
                .ok_or(SolMathError::Overflow)?;
            weighted_vv = weighted_vv
                .checked_add(fp_mul_i(gl_weights[node_idx], integrand_vv)?)
                .ok_or(SolMathError::Overflow)?;
        }

        total.probability = total
            .probability
            .checked_add(fp_mul_i(half, weighted_probability)?)
            .ok_or(SolMathError::Overflow)?;
        total.expectation_u = total
            .expectation_u
            .checked_add(fp_mul_i(half, weighted_u)?)
            .ok_or(SolMathError::Overflow)?;
        total.expectation_v = total
            .expectation_v
            .checked_add(fp_mul_i(half, weighted_v)?)
            .ok_or(SolMathError::Overflow)?;
        total.expectation_uu = total
            .expectation_uu
            .checked_add(fp_mul_i(half, weighted_uu)?)
            .ok_or(SolMathError::Overflow)?;
        total.expectation_uv = total
            .expectation_uv
            .checked_add(fp_mul_i(half, weighted_uv)?)
            .ok_or(SolMathError::Overflow)?;
        total.expectation_vv = total
            .expectation_vv
            .checked_add(fp_mul_i(half, weighted_vv)?)
            .ok_or(SolMathError::Overflow)?;
    }

    total.probability = clamp_prob(total.probability);
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SCALE;

    fn std_norm_pdf(x: f64) -> f64 {
        0.398_942_280_401_432_7 * (-0.5 * x * x).exp()
    }

    fn std_norm_cdf(x: f64) -> f64 {
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
        let mut sum = std_norm_pdf(left) + std_norm_pdf(right);
        for i in 1..n {
            let xi = left + h * i as f64;
            sum += if i % 2 == 0 {
                2.0 * std_norm_pdf(xi)
            } else {
                4.0 * std_norm_pdf(xi)
            };
        }
        (sum * h / 3.0).clamp(0.0, 1.0)
    }

    fn section_ref(vertices: &[(f64, f64); 3], u: f64) -> Option<(f64, f64)> {
        let mut hits = [0.0f64; 6];
        let mut count = 0usize;
        for idx in 0..3 {
            let (x1, y1) = vertices[idx];
            let (x2, y2) = vertices[(idx + 1) % 3];
            if u < x1.min(x2) - 1e-12 || u > x1.max(x2) + 1e-12 {
                continue;
            }
            if (x2 - x1).abs() < 1e-12 {
                hits[count] = y1;
                count += 1;
                hits[count] = y2;
                count += 1;
                continue;
            }
            let t = (u - x1) / (x2 - x1);
            hits[count] = y1 + t * (y2 - y1);
            count += 1;
        }
        if count < 2 {
            return None;
        }
        hits[..count].sort_by(|a, b| a.partial_cmp(b).unwrap());
        Some((hits[0], hits[count - 1]))
    }

    fn triangle_probability_ref(
        mean_u: f64,
        mean_v: f64,
        var_uu: f64,
        cov_uv: f64,
        var_vv: f64,
        vertices: &[(f64, f64); 3],
    ) -> f64 {
        let sigma_u = var_uu.sqrt();
        let var_v_cond = var_vv - cov_uv * cov_uv / var_uu;
        let sigma_v_cond = var_v_cond.sqrt();
        let x0 = vertices[0].0.min(vertices[1].0.min(vertices[2].0));
        let x2 = vertices[0].0.max(vertices[1].0.max(vertices[2].0));
        let n = 1024usize;
        let h = (x2 - x0) / n as f64;
        let mut sum = 0.0;
        for i in 0..=n {
            let u = x0 + h * i as f64;
            let coeff = if i == 0 || i == n {
                1.0
            } else if i % 2 == 0 {
                2.0
            } else {
                4.0
            };
            let Some((v_lo, v_hi)) = section_ref(vertices, u) else {
                continue;
            };
            let z_u = (u - mean_u) / sigma_u;
            let pdf_u = std_norm_pdf(z_u) / sigma_u;
            let cond_mean = mean_v + cov_uv * (u - mean_u) / var_uu;
            let strip = std_norm_cdf((v_hi - cond_mean) / sigma_v_cond)
                - std_norm_cdf((v_lo - cond_mean) / sigma_v_cond);
            sum += coeff * pdf_u * strip.max(0.0);
        }
        (sum * h / 3.0).clamp(0.0, 1.0)
    }

    fn triangle_region_moment_ref(
        mean_u: f64,
        mean_v: f64,
        var_uu: f64,
        cov_uv: f64,
        var_vv: f64,
        vertices: &[(f64, f64); 3],
    ) -> (f64, f64, f64, f64, f64, f64) {
        let sigma_u = var_uu.sqrt();
        let var_v_cond = var_vv - cov_uv * cov_uv / var_uu;
        let sigma_v_cond = var_v_cond.sqrt();
        let x0 = vertices[0].0.min(vertices[1].0.min(vertices[2].0));
        let x2 = vertices[0].0.max(vertices[1].0.max(vertices[2].0));
        let n = 1024usize;
        let h = (x2 - x0) / n as f64;
        let mut probability = 0.0_f64;
        let mut expectation_u = 0.0_f64;
        let mut expectation_v = 0.0_f64;
        let mut expectation_uu = 0.0_f64;
        let mut expectation_uv = 0.0_f64;
        let mut expectation_vv = 0.0_f64;
        for i in 0..=n {
            let u = x0 + h * i as f64;
            let coeff = if i == 0 || i == n {
                1.0
            } else if i % 2 == 0 {
                2.0
            } else {
                4.0
            };
            let Some((v_lo, v_hi)) = section_ref(vertices, u) else {
                continue;
            };
            let z_u = (u - mean_u) / sigma_u;
            let pdf_u = std_norm_pdf(z_u) / sigma_u;
            let cond_mean = mean_v + cov_uv * (u - mean_u) / var_uu;
            let z_hi = (v_hi - cond_mean) / sigma_v_cond;
            let z_lo = (v_lo - cond_mean) / sigma_v_cond;
            let cdf_hi = std_norm_cdf(z_hi);
            let cdf_lo = std_norm_cdf(z_lo);
            let prob_v = (cdf_hi - cdf_lo).max(0.0);
            if prob_v <= 0.0 {
                continue;
            }
            let pdf_hi = std_norm_pdf(z_hi);
            let pdf_lo = std_norm_pdf(z_lo);
            let v_truncated = cond_mean * prob_v + sigma_v_cond * (pdf_lo - pdf_hi);
            let second_moment_z = prob_v - z_hi * pdf_hi + z_lo * pdf_lo;
            let v_second = cond_mean * cond_mean * prob_v
                + 2.0 * cond_mean * sigma_v_cond * (pdf_lo - pdf_hi)
                + sigma_v_cond * sigma_v_cond * second_moment_z;
            let shell = coeff * pdf_u;
            probability += shell * prob_v;
            expectation_u += shell * u * prob_v;
            expectation_v += shell * v_truncated;
            expectation_uu += shell * u * u * prob_v;
            expectation_uv += shell * u * v_truncated;
            expectation_vv += shell * v_second;
        }
        let scale = h / 3.0;
        (
            (probability * scale).clamp(0.0, 1.0),
            expectation_u * scale,
            expectation_v * scale,
            expectation_uu * scale,
            expectation_uv * scale,
            expectation_vv * scale,
        )
    }

    fn planes_from_vertices(vertices: &[(f64, f64); 3]) -> [HalfPlane; 3] {
        let centroid = (
            (vertices[0].0 + vertices[1].0 + vertices[2].0) / 3.0,
            (vertices[0].1 + vertices[1].1 + vertices[2].1) / 3.0,
        );
        let mut planes = [HalfPlane {
            a_u: 0,
            a_v: 0,
            rhs: 0,
        }; 3];
        for idx in 0..3 {
            let (x1, y1) = vertices[idx];
            let (x2, y2) = vertices[(idx + 1) % 3];
            let mut a = y2 - y1;
            let mut b = -(x2 - x1);
            let mut rhs = a * x1 + b * y1;
            if a * centroid.0 + b * centroid.1 > rhs {
                a = -a;
                b = -b;
                rhs = -rhs;
            }
            planes[idx] = HalfPlane {
                a_u: (a * SCALE as f64).round() as i128,
                a_v: (b * SCALE as f64).round() as i128,
                rhs: (rhs * SCALE as f64).round() as i128,
            };
        }
        planes
    }

    #[test]
    fn triangle_probability_matches_reference_cases() {
        let vertices = [(-1.0, -0.5), (0.75, -0.8), (0.2, 1.1)];
        let planes = planes_from_vertices(&vertices);
        let got = triangle_probability(
            100_000_000_000,
            -50_000_000_000,
            180_000_000_000,
            40_000_000_000,
            220_000_000_000,
            planes,
        )
        .unwrap() as f64
            / SCALE as f64;
        let want = triangle_probability_ref(0.1, -0.05, 0.18, 0.04, 0.22, &vertices);
        assert!((got - want).abs() < 2.0e-6, "got={got} want={want}");
    }

    #[test]
    fn triangle_probability_matches_reference_generated_pack() {
        let mut state = 0xfeed_beef_cafe_babeu64;
        let mut max_err = 0.0f64;
        for _ in 0..160 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let ax = (((state >> 8) % 2_001) as i64 - 1_000) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let ay = (((state >> 10) % 2_001) as i64 - 1_000) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let bx = ax + 0.6 + ((state >> 12) % 700) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let by = ay - 0.4 + ((state >> 14) % 500) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let cx = (ax + bx) / 2.0 + (((state >> 16) % 600) as i64 - 300) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let cy = ay + 0.7 + ((state >> 18) % 900) as f64 / 1_000.0;
            let vertices = [(ax, ay), (bx, by), (cx, cy)];
            let planes = planes_from_vertices(&vertices);

            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let mean_u = (((state >> 20) % 801) as i64 - 400) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let mean_v = (((state >> 22) % 801) as i64 - 400) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let var_uu = 0.08 + ((state >> 24) % 180) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let var_vv = 0.08 + ((state >> 26) % 180) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let rho = (((state >> 28) % 1_201) as i64 - 600) as f64 / 1_000.0;
            let cov_uv = rho * (var_uu * var_vv).sqrt() * 0.8;

            let got = triangle_probability(
                (mean_u * SCALE as f64).round() as i128,
                (mean_v * SCALE as f64).round() as i128,
                (var_uu * SCALE as f64).round() as i128,
                (cov_uv * SCALE as f64).round() as i128,
                (var_vv * SCALE as f64).round() as i128,
                planes,
            )
            .unwrap() as f64
                / SCALE as f64;
            let want = triangle_probability_ref(mean_u, mean_v, var_uu, cov_uv, var_vv, &vertices);
            let err = (got - want).abs();
            if err > max_err {
                max_err = err;
            }
        }
        assert!(max_err < 2.0e-6, "max_err={max_err}");
    }

    #[test]
    fn triangle_region_moment_matches_reference_case() {
        let vertices = [(-1.0, -0.5), (0.75, -0.8), (0.2, 1.1)];
        let planes = planes_from_vertices(&vertices);
        let got = triangle_region_moment(
            100_000_000_000,
            -50_000_000_000,
            180_000_000_000,
            40_000_000_000,
            220_000_000_000,
            planes,
        )
        .unwrap();
        let want = triangle_region_moment_ref(0.1, -0.05, 0.18, 0.04, 0.22, &vertices);
        let got_tuple = (
            got.probability as f64 / SCALE as f64,
            got.expectation_u as f64 / SCALE as f64,
            got.expectation_v as f64 / SCALE as f64,
            got.expectation_uu as f64 / SCALE as f64,
            got.expectation_uv as f64 / SCALE as f64,
            got.expectation_vv as f64 / SCALE as f64,
        );
        assert!((got_tuple.0 - want.0).abs() < 2.0e-6);
        assert!((got_tuple.1 - want.1).abs() < 4.0e-6);
        assert!((got_tuple.2 - want.2).abs() < 4.0e-6);
        assert!((got_tuple.3 - want.3).abs() < 6.0e-6);
        assert!((got_tuple.4 - want.4).abs() < 6.0e-6);
        assert!((got_tuple.5 - want.5).abs() < 8.0e-6);
    }

    #[test]
    fn triangle_region_moment_matches_reference_generated_pack() {
        let mut state = 0x1234_5678_dead_beefu64;
        let mut max_prob_err = 0.0f64;
        let mut max_u_err = 0.0f64;
        let mut max_v_err = 0.0f64;
        let mut max_uu_err = 0.0f64;
        let mut max_uv_err = 0.0f64;
        let mut max_vv_err = 0.0f64;
        for _ in 0..96 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let ax = (((state >> 8) % 2_001) as i64 - 1_000) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let ay = (((state >> 10) % 2_001) as i64 - 1_000) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let bx = ax + 0.6 + ((state >> 12) % 700) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let by = ay - 0.4 + ((state >> 14) % 500) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let cx = (ax + bx) / 2.0 + (((state >> 16) % 600) as i64 - 300) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let cy = ay + 0.7 + ((state >> 18) % 900) as f64 / 1_000.0;
            let vertices = [(ax, ay), (bx, by), (cx, cy)];
            let planes = planes_from_vertices(&vertices);

            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let mean_u = (((state >> 20) % 801) as i64 - 400) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let mean_v = (((state >> 22) % 801) as i64 - 400) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let var_uu = 0.08 + ((state >> 24) % 180) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let var_vv = 0.08 + ((state >> 26) % 180) as f64 / 1_000.0;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let rho = (((state >> 28) % 1_201) as i64 - 600) as f64 / 1_000.0;
            let cov_uv = rho * (var_uu * var_vv).sqrt() * 0.8;

            let got = triangle_region_moment(
                (mean_u * SCALE as f64).round() as i128,
                (mean_v * SCALE as f64).round() as i128,
                (var_uu * SCALE as f64).round() as i128,
                (cov_uv * SCALE as f64).round() as i128,
                (var_vv * SCALE as f64).round() as i128,
                planes,
            )
            .unwrap();
            let want =
                triangle_region_moment_ref(mean_u, mean_v, var_uu, cov_uv, var_vv, &vertices);
            let got_tuple = (
                got.probability as f64 / SCALE as f64,
                got.expectation_u as f64 / SCALE as f64,
                got.expectation_v as f64 / SCALE as f64,
                got.expectation_uu as f64 / SCALE as f64,
                got.expectation_uv as f64 / SCALE as f64,
                got.expectation_vv as f64 / SCALE as f64,
            );
            max_prob_err = max_prob_err.max((got_tuple.0 - want.0).abs());
            max_u_err = max_u_err.max((got_tuple.1 - want.1).abs());
            max_v_err = max_v_err.max((got_tuple.2 - want.2).abs());
            max_uu_err = max_uu_err.max((got_tuple.3 - want.3).abs());
            max_uv_err = max_uv_err.max((got_tuple.4 - want.4).abs());
            max_vv_err = max_vv_err.max((got_tuple.5 - want.5).abs());
        }
        assert!(max_prob_err < 3.0e-6, "max_prob_err={max_prob_err}");
        assert!(max_u_err < 8.0e-6, "max_u_err={max_u_err}");
        assert!(max_v_err < 8.0e-6, "max_v_err={max_v_err}");
        assert!(max_uu_err < 1.2e-5, "max_uu_err={max_uu_err}");
        assert!(max_uv_err < 1.2e-5, "max_uv_err={max_uv_err}");
        assert!(max_vv_err < 1.4e-5, "max_vv_err={max_vv_err}");
    }

    #[test]
    fn phi2_matches_gl20_on_product_geometry() {
        use crate::bvn_resid_tables::*;

        // Barrier geometry from calibrated factor model (loadings l1,l2,l3).
        // Half-planes: l2*u + l3*v ≤ R, -(L-l2)*u + l3*v ≤ R, l2*u -(L-l3)*v ≤ R
        let l2: f64 = 0.5679724440670543;
        let l3: f64 = 0.6414271069443002;
        let l_sum: f64 = 1.7251306527083165;

        // UV covariance (daily × 63 step)
        let sigma_uu = 2.7879e-05 * 63.0;
        let sigma_uv = -2.849e-06 * 63.0;
        let sigma_vv = 4.2663e-05 * 63.0;

        let phi2_tables: [&[[i32; 64]; 64]; 3] = [
            &PHI2_RESID_SPY_QQQ,
            &PHI2_RESID_SPY_IWM,
            &PHI2_RESID_QQQ_IWM,
        ];
        // Pair signs: (SPY,QQQ)=+, (SPY,IWM)=−, (QQQ,IWM)=−
        let phi2_neg = [false, true, true];

        let mut max_err = 0.0f64;
        let mut feasible = 0u32;
        let mut n_compared = 0u32;
        for &barrier_log in &[0.0_f64, -0.22314, 0.02469] {
            // rhs = shift + (-L * barrier_log), sweep shift for a few factor values
            for &shift in &[0.0, 0.02, -0.03, 0.05, -0.01] {
                let rhs = shift + (-l_sum * barrier_log);
                let planes = [
                    HalfPlane {
                        a_u: (l2 * SCALE as f64).round() as i128,
                        a_v: (l3 * SCALE as f64).round() as i128,
                        rhs: (rhs * SCALE as f64).round() as i128,
                    },
                    HalfPlane {
                        a_u: (-(l_sum - l2) * SCALE as f64).round() as i128,
                        a_v: (l3 * SCALE as f64).round() as i128,
                        rhs: (rhs * SCALE as f64).round() as i128,
                    },
                    HalfPlane {
                        a_u: (l2 * SCALE as f64).round() as i128,
                        a_v: (-(l_sum - l3) * SCALE as f64).round() as i128,
                        rhs: (rhs * SCALE as f64).round() as i128,
                    },
                ];
                let mean_u = (shift * 0.1 * SCALE as f64).round() as i128;
                let mean_v = (shift * -0.05 * SCALE as f64).round() as i128;
                let var_uu = (sigma_uu * SCALE as f64).round() as i128;
                let cov_uv = (sigma_uv * SCALE as f64).round() as i128;
                let var_vv = (sigma_vv * SCALE as f64).round() as i128;

                if collect_vertices(planes).is_err() {
                    continue;
                }
                feasible += 1;

                let gl20 = triangle_probability(mean_u, mean_v, var_uu, cov_uv, var_vv, planes);
                let phi2 = triangle_probability_phi2(
                    mean_u,
                    mean_v,
                    var_uu,
                    cov_uv,
                    var_vv,
                    planes,
                    phi2_tables,
                    phi2_neg,
                );

                let g = gl20.unwrap_or_else(|err| {
                    panic!(
                        "GL20 failed on feasible product geometry: barrier_log={barrier_log} shift={shift} err={err:?}"
                    )
                });
                let p = phi2.unwrap_or_else(|err| {
                    panic!(
                        "phi2 failed on feasible product geometry: barrier_log={barrier_log} shift={shift} err={err:?}"
                    )
                });

                let gf = g as f64 / SCALE as f64;
                let pf = p as f64 / SCALE as f64;
                let err = (gf - pf).abs();
                if err > max_err {
                    max_err = err;
                }
                n_compared += 1;
            }
        }
        assert!(
            feasible == 8,
            "expected exactly 8 feasible product geometries from the fixed 3x5 sweep, got {feasible}"
        );
        assert!(
            n_compared == feasible,
            "expected all feasible geometries to compare successfully, compared {n_compared} of {feasible}"
        );
        assert!(
            max_err < 5.0e-4,
            "phi2 vs GL20 max_err={max_err} across {n_compared} cases"
        );
    }
}
