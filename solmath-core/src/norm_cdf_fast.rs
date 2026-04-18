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
use crate::SCALE_I;

const N: usize = 64;
const S6: i64 = 1_000_000;
const SHIFT: i64 = 1_000_000; // SCALE / S6

/// Φ(x) table on [-4, 4], 64 entries at SCALE_6.
/// Max Catmull-Rom interpolation error < 1.4×10⁻⁵.
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
/// Infallible Φ(x) at SCALE_6 in pure i64. Inputs clamped, indices clamped — cannot fail.
#[inline(always)]
pub fn norm_cdf_i64(x: i64) -> i64 {
    let x64 = x.clamp(DOMAIN_MIN_I64, DOMAIN_MAX_I64);
    let x_off = x64 - DOMAIN_MIN_I64;
    let ix_scaled = x_off * N_MINUS_1;
    let i0 = (ix_scaled / RANGE_I64).min(N as i64 - 2) as i32;
    let wi = ((ix_scaled % RANGE_I64) / FRAC_DIVISOR) as usize;
    let w = &CR_W[if wi < WN { wi } else { WN - 1 }];
    let n = N as i32;
    let p0 = PHI1_TABLE[(i0 - 1).clamp(0, n - 1) as usize] as i64;
    let p1 = PHI1_TABLE[i0.clamp(0, n - 1) as usize] as i64;
    let p2 = PHI1_TABLE[(i0 + 1).clamp(0, n - 1) as usize] as i64;
    let p3 = PHI1_TABLE[(i0 + 2).clamp(0, n - 1) as usize] as i64;
    cr_dot(w, p0, p1, p2, p3).clamp(0, S6)
}

/// φ(x) table on [-4, 4], 64 entries at SCALE_6.
const PDF1_TABLE: [i32; N] = [
    134, 221, 358, 571, 897, 1387, 2109, 3156, 4647, 6734, 9602, 13471, 18598, 25265, 33774, 44425,
    57501, 73235, 91783, 113188, 137353, 164010, 192708, 222806, 253484, 283774, 312601, 338848,
    361424, 379337, 391770, 398139, 398139, 391770, 379337, 361424, 338848, 312601, 283774, 253484,
    222806, 192708, 164010, 137353, 113188, 91783, 73235, 57501, 44425, 33774, 25265, 18598, 13471,
    9602, 6734, 4647, 3156, 2109, 1387, 897, 571, 358, 221, 134,
];

/// Infallible φ(x) at SCALE_6 in pure i64.
#[inline(always)]
pub fn norm_pdf_i64(x: i64) -> i64 {
    let x64 = x.clamp(DOMAIN_MIN_I64, DOMAIN_MAX_I64);
    let x_off = x64 - DOMAIN_MIN_I64;
    let ix_scaled = x_off * N_MINUS_1;
    let i0 = (ix_scaled / RANGE_I64).min(N as i64 - 2) as i32;
    let wi = ((ix_scaled % RANGE_I64) / FRAC_DIVISOR) as usize;
    let w = &CR_W[if wi < WN { wi } else { WN - 1 }];
    let n = N as i32;
    let p0 = PDF1_TABLE[(i0 - 1).clamp(0, n - 1) as usize] as i64;
    let p1 = PDF1_TABLE[i0.clamp(0, n - 1) as usize] as i64;
    let p2 = PDF1_TABLE[(i0 + 1).clamp(0, n - 1) as usize] as i64;
    let p3 = PDF1_TABLE[(i0 + 2).clamp(0, n - 1) as usize] as i64;
    cr_dot(w, p0, p1, p2, p3).clamp(0, S6)
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

    fn std_norm_cdf_ref(x: f64) -> f64 {
        0.5 * (1.0 + libm::erf(x / core::f64::consts::SQRT_2))
    }

    #[test]
    fn norm_cdf_fast_accuracy() {
        let mut max_err = 0.0_f64;
        let n = 2000;
        for i in 0..=n {
            let x = -4.0 + 8.0 * (i as f64 / n as f64);
            let x_fp = (x * SCALE_I as f64).round() as i128;
            let got = norm_cdf_fast(x_fp).unwrap() as f64 / SCALE_I as f64;
            let want = std_norm_cdf_ref(x);
            let err = (got - want).abs();
            if err > max_err {
                max_err = err;
            }
        }
        assert!(max_err < 2.0e-5, "max_err={max_err}");
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
}
