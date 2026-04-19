//! Fast univariate normal CDF via const table with i64 Catmull-Rom.
//!
//! Same pattern as `bvn_cdf_phi2table`: 64-entry Φ(x) on [-4, 4] at
//! SCALE_6, weight-LUT Catmull-Rom interpolation, pure i64 arithmetic.
//!
//! ~200 CU on SBF versus ~6,400 CU for `norm_cdf_poly` (i128 Horner).

use crate::bvn_cdf_phi2table::{
    cr_dot, CR_W, DOMAIN_MAX_I64, DOMAIN_MIN_I64, FRAC_DIVISOR, N_MINUS_1, RANGE_I64, WN,
};
use crate::error::SolMathError;

const N: usize = 64;
const S6: i64 = 1_000_000;
const SHIFT: i64 = 1_000_000; // SCALE / S6

/// Φ(x) table on [-4, 4], 64 entries at SCALE_6.
/// Empirically, the current SCALE_6 Catmull-Rom implementation stays under
/// 7×10⁻⁵ on a 20_001-point host sweep across [-4, 4]. Remaining error is
/// dominated by SCALE_6 table quantization and CR_W weight discretization.
const PHI1_TABLE: [i32; N] = [
    32, 54, 90, 148, 240, 383, 602, 932, 1422, 2137, 3165, 4618, 6640, 9407, 13134, 18075, 24519,
    32791, 43238, 56222, 72101, 91211, 113841, 140213, 170452, 204573, 242460, 283855, 328361,
    375447, 424468, 474687, 525313, 575532, 624553, 671639, 716145, 757540, 795427, 829548, 859787,
    886159, 908789, 927899, 943778, 956762, 967209, 975481, 981925, 986866, 990593, 993360, 995382,
    996835, 997863, 998578, 999068, 999398, 999617, 999760, 999852, 999910, 999946, 999968,
];

/// Fast univariate normal CDF via 1D Catmull-Rom table lookup.
///
/// Input `x` at SCALE (i128, narrowed to i64 after clamp).
/// Output at SCALE (i128).
///
/// ~200 CU on SBF. Pure i64 in the hot path.
/// Infallible Φ(x) at SCALE_6 in pure i64. Inputs saturate at ±4 and the
/// negative half uses exact Φ(-x) = 1 - Φ(x) reflection.
#[inline(always)]
pub fn norm_cdf_i64(x: i64) -> i64 {
    let x64 = x.clamp(DOMAIN_MIN_I64, DOMAIN_MAX_I64);
    if x64 < 0 {
        S6 - table_lookup_i64(&PHI1_TABLE, -x64)
    } else {
        table_lookup_i64(&PHI1_TABLE, x64)
    }
}

/// φ(x) table on [-4, 4], 64 entries at SCALE_6.
const PDF1_TABLE: [i32; N] = [
    134, 221, 358, 571, 897, 1387, 2109, 3156, 4647, 6734, 9602, 13471, 18598, 25265, 33774, 44425,
    57501, 73235, 91783, 113188, 137353, 164010, 192708, 222806, 253484, 283774, 312601, 338848,
    361424, 379337, 391770, 398139, 398139, 391770, 379337, 361424, 338848, 312601, 283774, 253484,
    222806, 192708, 164010, 137353, 113188, 91783, 73235, 57501, 44425, 33774, 25265, 18598, 13471,
    9602, 6734, 4647, 3156, 2109, 1387, 897, 571, 358, 221, 134,
];

#[inline(always)]
fn table_lookup_i64(table: &[i32; N], x64: i64) -> i64 {
    if x64 <= DOMAIN_MIN_I64 {
        return table[0] as i64;
    }
    if x64 >= DOMAIN_MAX_I64 {
        return table[N - 1] as i64;
    }

    let x_off = x64 - DOMAIN_MIN_I64;
    let ix_scaled = x_off * N_MINUS_1;
    let i0 = (ix_scaled / RANGE_I64).min(N as i64 - 2) as i32;
    let wi = ((ix_scaled % RANGE_I64) / FRAC_DIVISOR) as usize;
    let w = &CR_W[if wi < WN { wi } else { WN - 1 }];
    let n = N as i32;
    let p0 = table[(i0 - 1).clamp(0, n - 1) as usize] as i64;
    let p1 = table[i0.clamp(0, n - 1) as usize] as i64;
    let p2 = table[(i0 + 1).clamp(0, n - 1) as usize] as i64;
    let p3 = table[(i0 + 2).clamp(0, n - 1) as usize] as i64;
    cr_dot(w, p0, p1, p2, p3).clamp(0, S6)
}

/// Infallible φ(x) at SCALE_6 in pure i64.
#[inline(always)]
pub fn norm_pdf_i64(x: i64) -> i64 {
    let x64 = x.clamp(DOMAIN_MIN_I64, DOMAIN_MAX_I64).abs();
    table_lookup_i64(&PDF1_TABLE, x64)
}

/// Wrapped version returning `Result<i128>` at SCALE for API compatibility.
#[inline(always)]
pub fn norm_cdf_fast(x: i128) -> Result<i128, SolMathError> {
    let x64 = (x.clamp(DOMAIN_MIN_I64 as i128, DOMAIN_MAX_I64 as i128)) as i64;
    Ok(norm_cdf_i64(x64) as i128 * SHIFT as i128)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::SCALE_I;

    struct SweepStats {
        max_abs_err: f64,
        worst_x: f64,
    }

    struct RegionStats {
        interior_max_err: f64,
        interior_worst_x: f64,
        mid_max_err: f64,
        mid_worst_x: f64,
        tail_max_err: f64,
        tail_worst_x: f64,
    }

    fn std_norm_cdf_ref(x: f64) -> f64 {
        0.5 * (1.0 + libm::erf(x / core::f64::consts::SQRT_2))
    }

    fn sweep_stats(lo: f64, hi: f64, n: usize) -> SweepStats {
        let mut max_err = 0.0_f64;
        let mut worst_x = lo;
        for i in 0..=n {
            let x = lo + (hi - lo) * (i as f64 / n as f64);
            let x_fp = (x * SCALE_I as f64).round() as i128;
            let got = norm_cdf_fast(x_fp).unwrap() as f64 / SCALE_I as f64;
            let want = std_norm_cdf_ref(x);
            let err = (got - want).abs();
            if err > max_err {
                max_err = err;
                worst_x = x;
            }
        }
        SweepStats {
            max_abs_err: max_err,
            worst_x,
        }
    }

    fn region_stats(n: usize) -> RegionStats {
        let mut stats = RegionStats {
            interior_max_err: 0.0,
            interior_worst_x: 0.0,
            mid_max_err: 0.0,
            mid_worst_x: 0.0,
            tail_max_err: 0.0,
            tail_worst_x: 0.0,
        };

        for i in 0..=n {
            let x = -4.0 + 8.0 * (i as f64 / n as f64);
            let x_fp = (x * SCALE_I as f64).round() as i128;
            let got = norm_cdf_fast(x_fp).unwrap() as f64 / SCALE_I as f64;
            let want = std_norm_cdf_ref(x);
            let err = (got - want).abs();
            let ax = x.abs();

            if ax <= 3.0 {
                if err > stats.interior_max_err {
                    stats.interior_max_err = err;
                    stats.interior_worst_x = x;
                }
            } else if ax <= 3.5 {
                if err > stats.mid_max_err {
                    stats.mid_max_err = err;
                    stats.mid_worst_x = x;
                }
            } else if err > stats.tail_max_err {
                stats.tail_max_err = err;
                stats.tail_worst_x = x;
            }
        }

        stats
    }

    #[test]
    fn norm_cdf_fast_accuracy_dense_grid() {
        let stats = sweep_stats(-4.0, 4.0, 20_000);
        assert!(
            stats.max_abs_err < 7.0e-5,
            "overall max_err={} at x={}",
            stats.max_abs_err,
            stats.worst_x
        );
    }

    #[test]
    fn norm_cdf_fast_is_monotone_across_wide_sweep() {
        let mut prev = norm_cdf_fast(-8 * SCALE_I).unwrap();
        for i in 1..=40_000 {
            let x = -8.0 + 16.0 * (i as f64 / 40_000.0);
            let x_fp = (x * SCALE_I as f64).round() as i128;
            let got = norm_cdf_fast(x_fp).unwrap();
            assert!(got >= prev, "non-monotone at x={x}: prev={prev}, got={got}");
            prev = got;
        }
    }

    #[test]
    fn norm_cdf_fast_symmetry_is_exact() {
        for i in 0..=20_000 {
            let x = -4.0 + 8.0 * (i as f64 / 20_000.0);
            let x_fp = (x * SCALE_I as f64).round() as i128;
            let phi_x = norm_cdf_fast(x_fp).unwrap();
            let phi_neg_x = norm_cdf_fast(-x_fp).unwrap();
            assert_eq!(
                phi_x + phi_neg_x,
                SCALE_I,
                "symmetry violated at x={x}: phi_x={}, phi_neg_x={}",
                phi_x,
                phi_neg_x
            );
        }
    }

    #[test]
    fn norm_cdf_fast_midpoint_is_exact_half() {
        assert_eq!(norm_cdf_fast(0).unwrap(), SCALE_I / 2);
    }

    #[test]
    fn norm_cdf_fast_endpoints_are_exact_table_values() {
        assert_eq!(
            norm_cdf_fast(4 * SCALE_I).unwrap(),
            PHI1_TABLE[N - 1] as i128 * SHIFT as i128
        );
        assert_eq!(
            norm_cdf_fast(-4 * SCALE_I).unwrap(),
            PHI1_TABLE[0] as i128 * SHIFT as i128
        );
        assert_eq!(norm_pdf_i64(4 * SCALE_I as i64), PDF1_TABLE[N - 1] as i64);
        assert_eq!(norm_pdf_i64(-4 * SCALE_I as i64), PDF1_TABLE[0] as i64);
    }

    #[test]
    fn norm_cdf_fast_clamps_outside_domain() {
        let lo = norm_cdf_fast(-(DOMAIN_MAX_I64 as i128)).unwrap();
        let hi = norm_cdf_fast(DOMAIN_MAX_I64 as i128).unwrap();
        for multiple in [5_i128, 10, 20, 100] {
            assert_eq!(norm_cdf_fast(-multiple * SCALE_I).unwrap(), lo);
            assert_eq!(norm_cdf_fast(multiple * SCALE_I).unwrap(), hi);
        }
    }

    #[test]
    fn norm_cdf_fast_boundary_values() {
        let half = norm_cdf_fast(0).unwrap();
        assert!((half - SCALE_I / 2).abs() < 1000, "Φ(0) should be ~0.5");
        let lo = norm_cdf_fast(-5 * SCALE_I).unwrap();
        assert!(lo < SCALE_I / 10000, "Φ(-5) should be ~0");
        let hi = norm_cdf_fast(5 * SCALE_I).unwrap();
        assert!(hi > SCALE_I - SCALE_I / 10000, "Φ(5) should be ~1");
    }

    #[test]
    fn norm_pdf_i64_is_even_and_saturates_at_domain_edge() {
        assert_eq!(norm_pdf_i64(-DOMAIN_MAX_I64), norm_pdf_i64(DOMAIN_MAX_I64));
        assert_eq!(
            norm_pdf_i64(-10 * SCALE_I as i64),
            norm_pdf_i64(DOMAIN_MAX_I64)
        );
        assert_eq!(
            norm_pdf_i64(10 * SCALE_I as i64),
            norm_pdf_i64(DOMAIN_MAX_I64)
        );
        assert!(norm_pdf_i64(0) >= norm_pdf_i64(1_000_000_000_000));
    }

    #[test]
    fn norm_cdf_fast_accuracy_per_region() {
        let stats = region_stats(20_000);
        assert!(
            stats.interior_max_err < 7.0e-5,
            "interior_max_err={} at x={}",
            stats.interior_max_err,
            stats.interior_worst_x
        );
        assert!(
            stats.mid_max_err < 2.5e-6,
            "mid_max_err={} at x={}",
            stats.mid_max_err,
            stats.mid_worst_x
        );
        assert!(
            stats.tail_max_err < 2.0e-6,
            "tail_max_err={} at x={}",
            stats.tail_max_err,
            stats.tail_worst_x
        );
    }
}
