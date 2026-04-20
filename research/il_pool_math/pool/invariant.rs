//! Log-space invariant computation.
//!
//! V = Σ(w_k × ln(B_k_fp)) — the log-space invariant that should be
//! non-decreasing after every pool operation (fees make it increase).

use halcyon_common::constants::SCALE;
use halcyon_common::fp::token_to_fp;
use solmath_core::SolMathError;

/// Compute the log-space invariant of a weighted pool.
///
/// # Arguments
/// * `balances` - Pool balances (raw token units)
/// * `weights` - Token weights (at SCALE, u64)
/// * `decimals` - Token decimals
/// * `token_count` - Number of active tokens
///
/// # Returns
/// The invariant as a signed fixed-point value (i128 at SCALE).
/// ln(B_k_fp) can be negative for very small balances, so the sum is signed.
pub fn compute_log_invariant(
    balances: &[u64],
    weights: &[u64],
    decimals: &[u8],
    token_count: usize,
) -> Result<i128, SolMathError> {
    let mut sum: i128 = 0;

    for k in 0..token_count {
        // Normalise balance to fixed-point at SCALE
        let b_k_fp = token_to_fp(balances[k], decimals[k])?;

        // ln(B_k_fp) — signed because B_k_fp < SCALE is possible (tiny balance)
        let ln_b = solmath_core::ln_fixed_i(b_k_fp)?;

        // w_k × ln(B_k_fp) / SCALE — signed fixed-point multiply
        // w_k is at most SCALE (1e12), ln_b is at most ~72e12 (for max u64 balance).
        // Product fits in i128 (max ~7.2e25, well below i128::MAX ~1.7e38).
        let w_k = weights[k] as i128;
        let term = w_k.checked_mul(ln_b).ok_or(SolMathError::Overflow)? / (SCALE as i128);

        sum = sum.checked_add(term).ok_or(SolMathError::Overflow)?;
    }

    Ok(sum)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invariant_positive_for_reasonable_balances() {
        // 2-token pool with decent balances (> 1.0 at SCALE)
        let inv = compute_log_invariant(
            &[1_000_000, 2_000_000],
            &[500_000_000_000, 500_000_000_000],
            &[6, 6],
            2,
        )
        .unwrap();
        // Both balances at SCALE = 1e12, ln(1e12) ≈ 27.6 * SCALE
        // sum ≈ 0.5 * 27.6 + 0.5 * 28.3 ≈ 27.9 (positive)
        assert!(inv > 0);
    }

    #[test]
    fn invariant_increases_with_more_balance() {
        let inv1 = compute_log_invariant(
            &[1_000_000, 1_000_000],
            &[500_000_000_000, 500_000_000_000],
            &[6, 6],
            2,
        )
        .unwrap();
        let inv2 = compute_log_invariant(
            &[2_000_000, 1_000_000],
            &[500_000_000_000, 500_000_000_000],
            &[6, 6],
            2,
        )
        .unwrap();
        assert!(inv2 > inv1);
    }
}
