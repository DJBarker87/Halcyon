// Fenton-Wilkinson moment-matching: fit a lognormal to the first two moments
// of a positive random variable.
//
// Given E[X] = e1 > 0 and E[X²] = e2 > 0, returns (m̂, σ̂²) such that
// LogN(m̂, σ̂²) has the same first two moments.
//
// Cost: 2 ln calls ≈ 10K CU.

use crate::arithmetic::fp_mul_i;
use crate::error::SolMathError;
use crate::transcendental::ln_fixed_i;

/// Fenton-Wilkinson lognormal moment-match.
///
/// - **e1**: E[X] at SCALE, must be > 0.
/// - **e2**: E[X²] at SCALE, must be > 0 and > e1²/SCALE (positive variance).
/// - **Returns**: `(m_hat, sigma_sq)` at SCALE, where X ≈ LogN(m̂, σ̂²).
/// - **Errors**: `DomainError` if e1 ≤ 0, e2 ≤ 0, or variance is non-positive.
pub fn fenton_wilkinson_fit(e1: i128, e2: i128) -> Result<(i128, i128), SolMathError> {
    if e1 <= 0 || e2 <= 0 {
        return Err(SolMathError::DomainError);
    }

    // Check e2 > e1² / SCALE (ensures positive variance).
    // e1² / SCALE = fp_mul_i(e1, e1).
    let e1_sq = fp_mul_i(e1, e1)?;
    if e2 <= e1_sq {
        return Err(SolMathError::DomainError);
    }

    // σ̂² = ln(e2) - 2·ln(e1)
    let le1 = ln_fixed_i(e1 as u128)?;
    let le2 = ln_fixed_i(e2 as u128)?;
    let sigma_sq = le2 - 2 * le1;

    if sigma_sq <= 0 {
        return Err(SolMathError::DomainError);
    }

    // m̂ = ln(e1) - σ̂²/2
    let m_hat = le1 - sigma_sq / 2;

    Ok((m_hat, sigma_sq))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::SCALE_I;

    const SCALE: i128 = SCALE_I;

    #[test]
    fn basic_fit() {
        // LogN(0, 0.1): E[X] = exp(0.05) ≈ 1.05127, E[X²] = exp(0.2) ≈ 1.22140
        // At SCALE: e1 = 1_051_271_096_376, e2 = 1_221_402_758_160
        let e1 = 1_051_271_096_376i128;
        let e2 = 1_221_402_758_160i128;
        let (m, s2) = fenton_wilkinson_fit(e1, e2).unwrap();
        // Expected: m = 0 (not -σ²/2; m̂ = ln(E[X]) - σ̂²/2 = 0.05 - 0.05 = 0)
        // s2 = 0.1 * SCALE = 100_000_000_000
        let m_err = m.abs();
        let s2_err = (s2 - 100_000_000_000i128).abs();
        assert!(m_err <= 2, "m_hat error {m_err}");
        assert!(s2_err <= 2, "sigma_sq error {s2_err}");
    }

    #[test]
    fn rejects_negative_e1() {
        assert_eq!(
            fenton_wilkinson_fit(-SCALE, 2 * SCALE),
            Err(SolMathError::DomainError)
        );
    }

    #[test]
    fn rejects_zero_e1() {
        assert_eq!(
            fenton_wilkinson_fit(0, 2 * SCALE),
            Err(SolMathError::DomainError)
        );
    }

    #[test]
    fn rejects_non_positive_variance() {
        // e2 = e1² / SCALE → variance = 0
        let e1 = SCALE;
        let e2 = SCALE; // e1² / SCALE = SCALE
        assert_eq!(fenton_wilkinson_fit(e1, e2), Err(SolMathError::DomainError));
    }
}
