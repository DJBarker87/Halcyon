//! KI-region moment in pure i64 at SCALE_6.
//!
//! Same semantics as `worst_of_ki_moment` but all arithmetic in i64.
//! `exp6` (~1K CU) replaces `exp_fixed_i` (~6K CU). Cholesky factored
//! out of the inner loop. ~135K CU per 9-node evaluation versus ~900K+.

use crate::bvn_cdf_phi2table::{cr_dot, CR_W, WN};
use crate::error::SolMathError;
use crate::i64_math::sqrt6;

const S6: i64 = 1_000_000;
const SQRT_2_S6: i64 = 1_414_214; // √2 at SCALE_6

// ── exp table on [-1.5, 0] at SCALE_6, 256 entries ──────────────────

const EXP_N: usize = 256;
const EXP_LO: i64 = -1_500_000;
const EXP_RANGE: i64 = 1_500_000;
const EXP_N1: i64 = 255;
const EXP_FRAC_DIV: i64 = EXP_RANGE / WN as i64;

const EXP_TABLE: [i32; 256] = [
    223130, 224447, 225771, 227103, 228443, 229790, 231146, 232510, 233881, 235261, 236649, 238045,
    239450, 240862, 242283, 243713, 245151, 246597, 248052, 249515, 250987, 252468, 253958, 255456,
    256963, 258479, 260004, 261538, 263081, 264633, 266194, 267765, 269344, 270933, 272532, 274140,
    275757, 277384, 279020, 280666, 282322, 283988, 285663, 287349, 289044, 290749, 292465, 294190,
    295926, 297672, 299428, 301194, 302971, 304759, 306557, 308365, 310184, 312014, 313855, 315707,
    317569, 319443, 321328, 323223, 325130, 327048, 328978, 330919, 332871, 334835, 336810, 338797,
    340796, 342807, 344829, 346864, 348910, 350968, 353039, 355122, 357217, 359324, 361444, 363577,
    365722, 367879, 370050, 372233, 374429, 376638, 378860, 381095, 383344, 385605, 387880, 390169,
    392470, 394786, 397115, 399458, 401815, 404185, 406570, 408968, 411381, 413808, 416249, 418705,
    421175, 423660, 426160, 428674, 431203, 433747, 436306, 438880, 441469, 444074, 446694, 449329,
    451980, 454646, 457329, 460027, 462741, 465471, 468217, 470979, 473758, 476553, 479364, 482193,
    485037, 487899, 490777, 493673, 496585, 499515, 502462, 505426, 508408, 511408, 514425, 517460,
    520513, 523583, 526672, 529780, 532905, 536049, 539212, 542393, 545593, 548812, 552049, 555306,
    558583, 561878, 565193, 568527, 571881, 575255, 578649, 582063, 585497, 588951, 592426, 595921,
    599437, 602973, 606531, 610109, 613708, 617329, 620971, 624635, 628320, 632027, 635756, 639506,
    643279, 647074, 650892, 654732, 658595, 662480, 666389, 670320, 674275, 678253, 682254, 686279,
    690328, 694401, 698498, 702619, 706764, 710933, 715128, 719347, 723591, 727860, 732154, 736473,
    740818, 745189, 749585, 754008, 758456, 762931, 767432, 771959, 776514, 781095, 785703, 790338,
    795001, 799691, 804409, 809155, 813929, 818731, 823561, 828420, 833307, 838223, 843169, 848143,
    853147, 858180, 863243, 868336, 873459, 878612, 883796, 889010, 894255, 899530, 904837, 910176,
    915545, 920947, 926380, 931846, 937343, 942873, 948436, 954031, 959660, 965321, 971017, 976745,
    982508, 988304, 994135, 1000000,
];

/// exp(x) via 256-entry Catmull-Rom table on [-1.5, 0]. Pure i64. ~100 CU.
///
/// Input at SCALE_6. Clamped to table domain. Returns exp(x) at SCALE_6.
#[inline(always)]
fn exp6_fast(x: i64) -> i64 {
    let x = x.clamp(EXP_LO, 0);
    let x_off = x - EXP_LO;
    let ix_scaled = x_off * EXP_N1;
    let i0 = (ix_scaled / EXP_RANGE).min(EXP_N as i64 - 2) as i32;
    let wi = ((ix_scaled % EXP_RANGE) / EXP_FRAC_DIV) as usize;
    let w = &CR_W[if wi < WN { wi } else { WN - 1 }];
    let n = EXP_N as i32;
    let p0 = EXP_TABLE[(i0 - 1).clamp(0, n - 1) as usize] as i64;
    let p1 = EXP_TABLE[i0.clamp(0, n - 1) as usize] as i64;
    let p2 = EXP_TABLE[(i0 + 1).clamp(0, n - 1) as usize] as i64;
    let p3 = EXP_TABLE[(i0 + 2).clamp(0, n - 1) as usize] as i64;
    cr_dot(w, p0, p1, p2, p3).clamp(0, S6)
}

/// Affine log-return coordinate at SCALE_6.
#[derive(Debug, Clone, Copy)]
pub struct AffineCoord6 {
    pub constant: i64,
    pub u_coeff: i64,
    pub v_coeff: i64,
}

/// KI moment output at SCALE_6.
#[derive(Debug, Clone, Copy)]
pub struct KiMoment6 {
    /// P(any x_i ≤ barrier) at SCALE_6.
    pub ki_probability: i64,
    /// E[min(exp(x_i)) · 1_{KI}] at SCALE_6.
    pub worst_indicator: i64,
}

/// GH3 nodes at SCALE_6 (physicist convention).
const GH3_NODES_6: [i64; 3] = [-1_224_745, 0, 1_224_745];

/// GH3 weights / √π at SCALE_6.
const GH3_WPI_6: [i64; 3] = [166_667, 666_667, 166_667];

/// GH7 nodes at SCALE_6 (physicist convention).
const GH7_NODES_6: [i64; 7] = [
    -2_651_961, -1_673_552, -816_288, 0, 816_288, 1_673_552, 2_651_961,
];

/// GH7 weights / √π at SCALE_6.
const GH7_WPI_6: [i64; 7] = [548, 30_758, 240_094, 457_028, 240_094, 30_758, 548];

/// Pure i64 fixed-point multiply at SCALE_6.
///
/// For bounded inputs |a|, |b| ≤ 30·S6 (≤ 3e7): product ≤ 9e14, fits i64.
/// No i128 in the hot path.
#[inline(always)]
fn m6(a: i64, b: i64) -> i64 {
    // Split to avoid i64 overflow: a * (b / S6) + a * (b % S6) / S6
    // For |a| ≤ 3e7, |b/S6| ≤ 30: first term ≤ 9e8, fits.
    // a * (b%S6) ≤ 3e7 * 1e6 = 3e13, / S6 = 3e7, fits.
    let q = b / S6;
    let r = b % S6;
    a * q + a * r / S6
}

/// KI moment via 7×7 GH quadrature, pure i64 at SCALE_6.
///
/// All coordinate arithmetic in native i64 — no i128 intermediates
/// except inside `exp6` (which uses i128 for the Taylor multiply).
///
/// `mean_u`, `mean_v`: conditional (u,v) mean at SCALE_6.
/// `l11`, `l21`, `l22`: precomputed Cholesky at SCALE_6.
/// `barrier`: KI barrier in log space at SCALE_6.
/// `coords`: affine maps from (u,v) to log returns at SCALE_6.
#[inline(always)]
pub fn ki_moment_i64(
    mean_u: i64,
    mean_v: i64,
    l11: i64,
    l21: i64,
    l22: i64,
    barrier: i64,
    coords: [AffineCoord6; 3],
) -> KiMoment6 {
    let mut ki_prob_acc: i64 = 0;
    let mut worst_acc: i64 = 0;

    // √2 · l11 / S6 and √2 · l21 / S6, √2 · l22 / S6 precomputed
    let sl11 = m6(SQRT_2_S6, l11);
    let sl21 = m6(SQRT_2_S6, l21);
    let sl22 = m6(SQRT_2_S6, l22);

    for i in 0..7 {
        let z1 = GH7_NODES_6[i];
        let w1 = GH7_WPI_6[i];
        let u = mean_u + m6(sl11, z1);

        let x_u = [
            coords[0].constant + m6(coords[0].u_coeff, u),
            coords[1].constant + m6(coords[1].u_coeff, u),
            coords[2].constant + m6(coords[2].u_coeff, u),
        ];

        let v_base = mean_v + m6(sl21, z1);

        for j in 0..7 {
            let z2 = GH7_NODES_6[j];
            let w2 = GH7_WPI_6[j];
            let v = v_base + m6(sl22, z2);

            let x0 = x_u[0] + m6(coords[0].v_coeff, v);
            let x1 = x_u[1] + m6(coords[1].v_coeff, v);
            let x2 = x_u[2] + m6(coords[2].v_coeff, v);

            if x0 <= barrier || x1 <= barrier || x2 <= barrier {
                let weight = m6(w1, w2);
                ki_prob_acc += weight;
                let worst_level = exp6_fast(x0.min(x1).min(x2));
                worst_acc += m6(weight, worst_level);
            }
        }
    }

    KiMoment6 {
        ki_probability: ki_prob_acc.clamp(0, S6),
        worst_indicator: worst_acc.max(0),
    }
}

/// KI moment via 3×3 GH quadrature. 9 points, fully unrolled. Pure i64.
#[inline(always)]
pub fn ki_moment_i64_gh3(
    mean_u: i64,
    mean_v: i64,
    l11: i64,
    l21: i64,
    l22: i64,
    barrier: i64,
    coords: [AffineCoord6; 3],
) -> KiMoment6 {
    let sl11 = m6(SQRT_2_S6, l11);
    let sl21 = m6(SQRT_2_S6, l21);
    let sl22 = m6(SQRT_2_S6, l22);

    let mut kp: i64 = 0;
    let mut wa: i64 = 0;

    // Macro: one GH point (i, j) → compute coords, check KI, accumulate.
    macro_rules! pt {
        ($zi:expr, $wi:expr, $zj:expr, $wj:expr) => {{
            let u = mean_u + m6(sl11, $zi);
            let v = mean_v + m6(sl21, $zi) + m6(sl22, $zj);
            let x0 = coords[0].constant + m6(coords[0].u_coeff, u) + m6(coords[0].v_coeff, v);
            let x1 = coords[1].constant + m6(coords[1].u_coeff, u) + m6(coords[1].v_coeff, v);
            let x2 = coords[2].constant + m6(coords[2].u_coeff, u) + m6(coords[2].v_coeff, v);
            if x0 <= barrier || x1 <= barrier || x2 <= barrier {
                let w = m6($wi, $wj);
                kp += w;
                wa += m6(w, exp6_fast(x0.min(x1).min(x2)));
            }
        }};
    }

    // GH3 nodes: [-1.2247, 0, 1.2247], weights/√π: [1/6, 2/3, 1/6]
    const Z0: i64 = -1_224_745;
    const Z1: i64 = 0;
    const Z2: i64 = 1_224_745;
    const W0: i64 = 166_667;
    const W1: i64 = 666_667;
    const W2: i64 = 166_667;

    pt!(Z0, W0, Z0, W0);
    pt!(Z0, W0, Z1, W1);
    pt!(Z0, W0, Z2, W2);
    pt!(Z1, W1, Z0, W0);
    pt!(Z1, W1, Z1, W1);
    pt!(Z1, W1, Z2, W2);
    pt!(Z2, W2, Z0, W0);
    pt!(Z2, W2, Z1, W1);
    pt!(Z2, W2, Z2, W2);

    KiMoment6 {
        ki_probability: kp.clamp(0, S6),
        worst_indicator: wa.max(0),
    }
}

/// Precompute Cholesky from (u,v) covariance at SCALE_6.
/// Returns (l11, l21, l22) or error if not positive definite.
#[inline]
pub fn cholesky6(var_uu: i64, cov_uv: i64, var_vv: i64) -> Result<(i64, i64, i64), SolMathError> {
    if var_uu <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let l11 = sqrt6(var_uu)?;
    let l21 = (cov_uv as i128 * S6 as i128 / l11 as i128) as i64;
    let cond_var = var_vv - (l21 as i128 * l21 as i128 / S6 as i128) as i64;
    if cond_var <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let l22 = sqrt6(cond_var)?;
    Ok((l11, l21, l22))
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    #[test]
    fn ki_moment_positive_probability_below_barrier() {
        // Mean near barrier → significant KI probability
        let (l11, l21, l22) = cholesky6(1756, -179, 2688).unwrap();
        let coords = [
            AffineCoord6 {
                constant: -50_000,
                u_coeff: -500_000,
                v_coeff: -400_000,
            },
            AffineCoord6 {
                constant: -50_000,
                u_coeff: 500_000,
                v_coeff: -400_000,
            },
            AffineCoord6 {
                constant: -50_000,
                u_coeff: -500_000,
                v_coeff: 600_000,
            },
        ];
        let barrier = -223_144; // ln(0.8) at SCALE_6
        let m = ki_moment_i64(0, 0, l11, l21, l22, barrier, coords);
        assert!(m.ki_probability > 0, "should have nonzero KI prob");
        assert!(m.worst_indicator > 0, "should have nonzero worst indicator");
        assert!(m.ki_probability <= S6);
    }

    #[test]
    fn ki_moment_zero_when_far_above_barrier() {
        let (l11, l21, l22) = cholesky6(1756, -179, 2688).unwrap();
        // All names well above barrier
        let coords = [
            AffineCoord6 {
                constant: 200_000,
                u_coeff: -500_000,
                v_coeff: -400_000,
            },
            AffineCoord6 {
                constant: 200_000,
                u_coeff: 500_000,
                v_coeff: -400_000,
            },
            AffineCoord6 {
                constant: 200_000,
                u_coeff: -500_000,
                v_coeff: 600_000,
            },
        ];
        let barrier = -223_144;
        let m = ki_moment_i64(0, 0, l11, l21, l22, barrier, coords);
        assert_eq!(m.ki_probability, 0, "no KI when far above barrier");
        assert_eq!(m.worst_indicator, 0);
    }
}
