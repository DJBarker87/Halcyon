// Lognormal call pricing: Black-Scholes with r=0, T=1, parameterised by
// forward F, strike K, and total variance v = σ².
//
// Hybrid path: standard-precision ln (5K CU each) for d1 numerator,
// HP norm_cdf + HP multiply for the final price. Rounding of d1/d2
// from standard ln is absorbed by the CDF's smoothness — 1 ULP error
// on ln(F/K) at SCALE produces ~1 ULP error on Φ(d) at SCALE_HP.
//
// lognormal_call:        ~35K CU, ≤ 10 ULP
// lognormal_spread_call: ~55K CU, ≤ 10 ULP (shares sqrt_v and ln_f)

use crate::arithmetic::{fp_div_i, fp_mul_i_round, fp_sqrt};
use crate::constants::*;
use crate::error::SolMathError;
use crate::hp::{downscale_hp_to_std_i, fp_mul_hp_i, norm_cdf_poly_hp, upscale_std_to_hp};
use crate::normal::norm_cdf_poly;
use crate::transcendental::ln_fixed_i;

/// Black-Scholes call price with r=0, T=1 (hybrid path).
///
/// Accepts and returns values at `SCALE` (1e12). Uses standard-precision ln
/// for d1/d2 computation, then HP norm_cdf and HP multiply for the final
/// price. ~35K CU on Solana, ≤ 10 ULP accuracy.
///
/// - **forward**: F at SCALE.
/// - **strike**: K at SCALE.
/// - **variance**: v = σ² at SCALE.
/// - **Returns**: call price at SCALE, clamped to ≥ 0.
pub fn lognormal_call(forward: i128, strike: i128, variance: i128) -> Result<i128, SolMathError> {
    if forward <= 0 {
        return Ok(0);
    }
    if strike <= 0 {
        return Ok(forward);
    }
    if variance <= 0 {
        return Ok(if forward > strike {
            forward - strike
        } else {
            0
        });
    }

    // Standard-precision sqrt and ln (~5K CU each, ~3K for sqrt)
    let sqrt_v = fp_sqrt(variance as u128)? as i128;
    if sqrt_v == 0 {
        return Ok(if forward > strike {
            forward - strike
        } else {
            0
        });
    }

    let ln_f = ln_fixed_i(forward as u128)?;
    let ln_k = ln_fixed_i(strike as u128)?;
    let ln_fk = ln_f - ln_k;

    // d1 = (ln(F/K) + v/2) / sqrt(v) at SCALE
    let d1 = fp_div_i(ln_fk + variance / 2, sqrt_v)?;
    let d2 = d1 - sqrt_v;

    // Upscale d1, d2 to HP for accurate CDF evaluation
    // d1 at SCALE → d1 * 1000 at SCALE_HP. The ±1 ULP error on d1 at SCALE
    // becomes ±1000 at SCALE_HP, which is < 1e-12 — well within CDF accuracy.
    let d1_hp = d1 * HP_TO_STD;
    let d2_hp = d2 * HP_TO_STD;

    // HP CDF (~10K each) + HP multiply (~2K each)
    let f_hp = upscale_std_to_hp(forward as u128)?;
    let k_hp = upscale_std_to_hp(strike as u128)?;

    let phi_d1 = norm_cdf_poly_hp(d1_hp)?;
    let phi_d2 = norm_cdf_poly_hp(d2_hp)?;

    let call_hp = fp_mul_hp_i(f_hp, phi_d1)? - fp_mul_hp_i(k_hp, phi_d2)?;

    let call = downscale_hp_to_std_i(call_hp);
    Ok(if call > 0 { call } else { 0 })
}

/// Call spread: lognormal_call(F, K_lo, v) - lognormal_call(F, K_hi, v).
///
/// Hybrid path sharing sqrt(v) and ln(F). ~55K CU, ≤ 10 ULP.
///
/// - **forward**: F at SCALE.
/// - **strike_lo**: lower strike (deductible) at SCALE.
/// - **strike_hi**: upper strike (cap) at SCALE.
/// - **variance**: v = σ² at SCALE.
/// - **Returns**: spread price at SCALE, clamped to ≥ 0.
pub fn lognormal_spread_call(
    forward: i128,
    strike_lo: i128,
    strike_hi: i128,
    variance: i128,
) -> Result<i128, SolMathError> {
    if forward <= 0 {
        return Ok(0);
    }
    if strike_hi <= strike_lo {
        return Ok(0);
    }

    if variance <= 0 {
        let call_lo = if forward > strike_lo {
            forward - strike_lo
        } else {
            0
        };
        let call_hi = if forward > strike_hi {
            forward - strike_hi
        } else {
            0
        };
        let spread = call_lo - call_hi;
        return Ok(if spread > 0 { spread } else { 0 });
    }

    // Shared: sqrt(v), ln(F), v/2
    let sqrt_v = fp_sqrt(variance as u128)? as i128;
    if sqrt_v == 0 {
        let call_lo = if forward > strike_lo {
            forward - strike_lo
        } else {
            0
        };
        let call_hi = if forward > strike_hi {
            forward - strike_hi
        } else {
            0
        };
        let spread = call_lo - call_hi;
        return Ok(if spread > 0 { spread } else { 0 });
    }

    let ln_f = ln_fixed_i(forward as u128)?;
    let half_var = variance / 2;
    let sqrt_v_hp = sqrt_v * HP_TO_STD;

    let f_hp = upscale_std_to_hp(forward as u128)?;

    // --- Call at lower strike ---
    let call_lo_hp = if strike_lo <= 0 {
        f_hp - (strike_lo as i128 * HP_TO_STD)
    } else {
        let ln_k_lo = ln_fixed_i(strike_lo as u128)?;
        let d1_lo = fp_div_i(ln_f - ln_k_lo + half_var, sqrt_v)?;
        let d2_lo = d1_lo - sqrt_v;
        let k_lo_hp = upscale_std_to_hp(strike_lo as u128)?;
        let c = fp_mul_hp_i(f_hp, norm_cdf_poly_hp(d1_lo * HP_TO_STD)?)?
            - fp_mul_hp_i(k_lo_hp, norm_cdf_poly_hp(d2_lo * HP_TO_STD)?)?;
        if c > 0 {
            c
        } else {
            0
        }
    };

    // --- Call at upper strike ---
    let call_hi_hp = if strike_hi <= 0 {
        f_hp - (strike_hi as i128 * HP_TO_STD)
    } else {
        let ln_k_hi = ln_fixed_i(strike_hi as u128)?;
        let d1_hi = fp_div_i(ln_f - ln_k_hi + half_var, sqrt_v)?;
        let d2_hi = d1_hi - sqrt_v;
        let k_hi_hp = upscale_std_to_hp(strike_hi as u128)?;
        let c = fp_mul_hp_i(f_hp, norm_cdf_poly_hp(d1_hi * HP_TO_STD)?)?
            - fp_mul_hp_i(k_hi_hp, norm_cdf_poly_hp(d2_hi * HP_TO_STD)?)?;
        if c > 0 {
            c
        } else {
            0
        }
    };

    let spread_hp = call_lo_hp - call_hi_hp;
    let spread = downscale_hp_to_std_i(spread_hp);
    Ok(if spread > 0 { spread } else { 0 })
}

/// Black-Scholes call price with r=0, T=1 (fully standard precision).
///
/// Same as `lognormal_call` but uses `norm_cdf_poly` (standard precision, ~6K CU)
/// instead of `norm_cdf_poly_hp` (~10K CU). All arithmetic at SCALE (1e12).
///
/// - **forward**: F at SCALE.
/// - **strike**: K at SCALE.
/// - **variance**: v = σ² at SCALE.
/// - **Returns**: call price at SCALE, clamped to ≥ 0.
pub fn lognormal_call_std(
    forward: i128,
    strike: i128,
    variance: i128,
) -> Result<i128, SolMathError> {
    if forward <= 0 {
        return Ok(0);
    }
    if strike <= 0 {
        return Ok(forward);
    }
    if variance <= 0 {
        return Ok(if forward > strike {
            forward - strike
        } else {
            0
        });
    }

    let sqrt_v = fp_sqrt(variance as u128)? as i128;
    if sqrt_v == 0 {
        return Ok(if forward > strike {
            forward - strike
        } else {
            0
        });
    }

    // ln(F/K) via ratio-then-ln: one ln call instead of two, and no
    // subtraction cancellation when F ≈ K.
    let ln_fk = ln_fixed_i(fp_div_i(forward, strike)? as u128)?;

    let d1 = fp_div_i(ln_fk + variance / 2, sqrt_v)?;
    let d2 = d1 - sqrt_v;

    let phi_d1 = norm_cdf_poly(d1)?;
    let phi_d2 = norm_cdf_poly(d2)?;

    let call = fp_mul_i_round(forward, phi_d1)? - fp_mul_i_round(strike, phi_d2)?;
    Ok(if call > 0 { call } else { 0 })
}

/// Call spread at fully standard precision.
///
/// Same as `lognormal_spread_call` but uses `norm_cdf_poly` and `fp_mul_i_round`
/// instead of HP variants. All arithmetic at SCALE (1e12).
///
/// - **forward**: F at SCALE.
/// - **strike_lo**: lower strike (deductible) at SCALE.
/// - **strike_hi**: upper strike (cap) at SCALE.
/// - **variance**: v = σ² at SCALE.
/// - **Returns**: spread price at SCALE, clamped to ≥ 0.
pub fn lognormal_spread_call_std(
    forward: i128,
    strike_lo: i128,
    strike_hi: i128,
    variance: i128,
) -> Result<i128, SolMathError> {
    if forward <= 0 {
        return Ok(0);
    }
    if strike_hi <= strike_lo {
        return Ok(0);
    }

    if variance <= 0 {
        let call_lo = if forward > strike_lo {
            forward - strike_lo
        } else {
            0
        };
        let call_hi = if forward > strike_hi {
            forward - strike_hi
        } else {
            0
        };
        let spread = call_lo - call_hi;
        return Ok(if spread > 0 { spread } else { 0 });
    }

    let sqrt_v = fp_sqrt(variance as u128)? as i128;
    if sqrt_v == 0 {
        let call_lo = if forward > strike_lo {
            forward - strike_lo
        } else {
            0
        };
        let call_hi = if forward > strike_hi {
            forward - strike_hi
        } else {
            0
        };
        let spread = call_lo - call_hi;
        return Ok(if spread > 0 { spread } else { 0 });
    }

    let half_var = variance / 2;

    // --- Call at lower strike ---
    // ln(F/K) via ratio-then-ln: saves one ln call vs ln(F)-ln(K),
    // and eliminates subtraction cancellation when F ≈ K.
    let call_lo = if strike_lo <= 0 {
        forward - strike_lo
    } else {
        let ln_fk_lo = ln_fixed_i(fp_div_i(forward, strike_lo)? as u128)?;
        let d1_lo = fp_div_i(ln_fk_lo + half_var, sqrt_v)?;
        let d2_lo = d1_lo - sqrt_v;
        let c = fp_mul_i_round(forward, norm_cdf_poly(d1_lo)?)?
            - fp_mul_i_round(strike_lo, norm_cdf_poly(d2_lo)?)?;
        if c > 0 {
            c
        } else {
            0
        }
    };

    // --- Call at upper strike ---
    let call_hi = if strike_hi <= 0 {
        forward - strike_hi
    } else {
        let ln_fk_hi = ln_fixed_i(fp_div_i(forward, strike_hi)? as u128)?;
        let d1_hi = fp_div_i(ln_fk_hi + half_var, sqrt_v)?;
        let d2_hi = d1_hi - sqrt_v;
        let c = fp_mul_i_round(forward, norm_cdf_poly(d1_hi)?)?
            - fp_mul_i_round(strike_hi, norm_cdf_poly(d2_hi)?)?;
        if c > 0 {
            c
        } else {
            0
        }
    };

    let spread = call_lo - call_hi;
    Ok(if spread > 0 { spread } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: i128 = SCALE_I;

    #[test]
    fn atm_call() {
        let call = lognormal_call(S, S, S / 25).unwrap();
        let expected_approx = 79_655_674_554i128;
        let diff = (call - expected_approx).abs();
        assert!(
            diff <= 5,
            "ATM call = {call}, expected ~{expected_approx}, diff {diff}"
        );
    }

    #[test]
    fn deep_itm_call() {
        let call = lognormal_call(5 * S, S, S / 25).unwrap();
        assert!(
            call > 3_999_000_000_000,
            "deep ITM call should be near 4.0, got {call}"
        );
    }

    #[test]
    fn deep_otm_call() {
        let call = lognormal_call(S, 5 * S, S / 25).unwrap();
        assert!(
            call < 1_000_000,
            "deep OTM call should be near 0, got {call}"
        );
    }

    #[test]
    fn zero_variance_itm() {
        assert_eq!(lognormal_call(2 * S, S, 0).unwrap(), S);
    }

    #[test]
    fn zero_variance_otm() {
        assert_eq!(lognormal_call(S, 2 * S, 0).unwrap(), 0);
    }

    #[test]
    fn negative_strike() {
        assert_eq!(lognormal_call(S, -S, S / 25).unwrap(), S);
    }

    #[test]
    fn zero_forward() {
        assert_eq!(lognormal_call(0, S, S / 25).unwrap(), 0);
    }

    #[test]
    fn spread_basic() {
        let f = S;
        let k_lo = 900_000_000_000i128;
        let k_hi = 1_100_000_000_000i128;
        let v = S / 25;
        let spread = lognormal_spread_call(f, k_lo, k_hi, v).unwrap();
        let c_lo = lognormal_call(f, k_lo, v).unwrap();
        let c_hi = lognormal_call(f, k_hi, v).unwrap();
        let expected = c_lo - c_hi;
        let diff = (spread - expected).abs();
        assert!(
            diff <= 1,
            "spread {spread} vs individual diff {expected}, diff {diff}"
        );
    }

    #[test]
    fn spread_hi_le_lo() {
        assert_eq!(lognormal_spread_call(S, S, S / 2, S / 25).unwrap(), 0);
    }

    #[test]
    fn spread_zero_forward() {
        assert_eq!(lognormal_spread_call(0, S / 2, S, S / 25).unwrap(), 0);
    }

    // ── Standard-precision variant tests ──

    #[test]
    fn std_call_close_to_hybrid() {
        // ATM
        let hybrid = lognormal_call(S, S, S / 25).unwrap();
        let std = lognormal_call_std(S, S, S / 25).unwrap();
        let diff = (hybrid - std).abs();
        assert!(diff <= 5, "ATM: hybrid={hybrid} std={std} diff={diff}");
    }

    #[test]
    fn std_call_itm() {
        let hybrid = lognormal_call(5 * S, S, S / 25).unwrap();
        let std = lognormal_call_std(5 * S, S, S / 25).unwrap();
        let diff = (hybrid - std).abs();
        assert!(diff <= 5, "ITM: hybrid={hybrid} std={std} diff={diff}");
    }

    #[test]
    fn std_call_otm() {
        let hybrid = lognormal_call(S, 5 * S, S / 25).unwrap();
        let std = lognormal_call_std(S, 5 * S, S / 25).unwrap();
        let diff = (hybrid - std).abs();
        assert!(diff <= 5, "OTM: hybrid={hybrid} std={std} diff={diff}");
    }

    #[test]
    fn std_call_edge_cases() {
        assert_eq!(lognormal_call_std(0, S, S / 25).unwrap(), 0);
        assert_eq!(lognormal_call_std(S, -S, S / 25).unwrap(), S);
        assert_eq!(lognormal_call_std(2 * S, S, 0).unwrap(), S);
        assert_eq!(lognormal_call_std(S, 2 * S, 0).unwrap(), 0);
    }

    #[test]
    fn std_spread_close_to_hybrid() {
        let f = S;
        let k_lo = 900_000_000_000i128;
        let k_hi = 1_100_000_000_000i128;
        let v = S / 25;
        let hybrid = lognormal_spread_call(f, k_lo, k_hi, v).unwrap();
        let std = lognormal_spread_call_std(f, k_lo, k_hi, v).unwrap();
        let diff = (hybrid - std).abs();
        assert!(diff <= 5, "spread: hybrid={hybrid} std={std} diff={diff}");
    }

    #[test]
    fn std_spread_insurance_typical() {
        // Typical insurance scenario: F≈1, small deductible, moderate cap
        let f = 1_003_000_000_000i128; // 1.003
        let k_lo = 1_005_000_000_000i128; // 1.005 (0.5% deductible)
        let k_hi = 1_100_000_000_000i128; // 1.100 (10% cap)
        let v = 30_000_000i128; // 0.00003 (very low vol)
        let hybrid = lognormal_spread_call(f, k_lo, k_hi, v).unwrap();
        let std = lognormal_spread_call_std(f, k_lo, k_hi, v).unwrap();
        let diff = (hybrid - std).abs();
        assert!(
            diff <= 5,
            "insurance: hybrid={hybrid} std={std} diff={diff}"
        );
    }

    #[test]
    fn std_spread_edge_cases() {
        assert_eq!(lognormal_spread_call_std(0, S / 2, S, S / 25).unwrap(), 0);
        assert_eq!(lognormal_spread_call_std(S, S, S / 2, S / 25).unwrap(), 0);
    }

    #[test]
    fn std_spread_grid() {
        // Quick grid: 20 vectors across various regimes
        let forwards = [500_000_000_000i128, S, 2 * S, 5 * S];
        let variances = [S / 100, S / 25, S / 4, S];
        let mut max_diff: i128 = 0;
        let mut count = 0;
        for &f in &forwards {
            for &v in &variances {
                let k_lo = f * 8 / 10;
                let k_hi = f * 12 / 10;
                let hybrid = lognormal_spread_call(f, k_lo, k_hi, v).unwrap();
                let std = lognormal_spread_call_std(f, k_lo, k_hi, v).unwrap();
                let diff = (hybrid - std).abs();
                max_diff = max_diff.max(diff);
                count += 1;
            }
        }
        assert!(
            max_diff <= 10,
            "grid ({count} vectors): max diff={max_diff}"
        );
    }
}
