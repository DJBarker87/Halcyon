//! Proportional join computation.
//!
//! Each token deposit = ceil(balance[k] * desired_lp / supply).
//! Rounding UP means user pays more — pool-favourable.

use solmath_core::SolMathError;

/// Compute deposit amounts for a proportional join.
///
/// # Returns
/// Vector of deposit amounts per token (raw, ceil-rounded).
pub fn compute_proportional_join(
    balances: &[u64],
    desired_lp: u64,
    supply: u64,
) -> Result<Vec<u64>, SolMathError> {
    balances
        .iter()
        .map(|&b_k| solmath_core::mul_div_ceil(b_k, desired_lp, supply))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proportional_join_rounds_up() {
        // 2-token pool: balances [100, 200], supply 1000, want 1 LP
        let amounts = compute_proportional_join(&[100, 200], 1, 1000).unwrap();
        // ceil(100 * 1 / 1000) = ceil(0.1) = 1
        // ceil(200 * 1 / 1000) = ceil(0.2) = 1
        assert_eq!(amounts, vec![1, 1]);
    }

    #[test]
    fn proportional_join_exact_divisible() {
        // Exact: balances [1000, 2000], supply 100, want 10
        let amounts = compute_proportional_join(&[1000, 2000], 10, 100).unwrap();
        assert_eq!(amounts, vec![100, 200]);
    }
}
