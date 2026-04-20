//! Proportional exit computation.
//!
//! Each token withdrawal = floor(balance[k] * burn_amount / supply).
//! Rounding DOWN means user gets less — pool-favourable.

use solmath_core::SolMathError;

/// Compute withdrawal amounts for a proportional exit.
///
/// # Returns
/// Vector of withdrawal amounts per token (raw, floor-rounded).
pub fn compute_proportional_exit(
    balances: &[u64],
    burn_amount: u64,
    supply: u64,
) -> Result<Vec<u64>, SolMathError> {
    balances
        .iter()
        .map(|&b_k| solmath_core::mul_div_floor(b_k, burn_amount, supply))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proportional_exit_rounds_down() {
        // 2-token pool: balances [100, 200], supply 1000, burn 1
        let amounts = compute_proportional_exit(&[100, 200], 1, 1000).unwrap();
        // floor(100 * 1 / 1000) = floor(0.1) = 0
        // floor(200 * 1 / 1000) = floor(0.2) = 0
        assert_eq!(amounts, vec![0, 0]);
    }

    #[test]
    fn proportional_exit_exact_divisible() {
        // Exact: balances [1000, 2000], supply 100, burn 10
        let amounts = compute_proportional_exit(&[1000, 2000], 10, 100).unwrap();
        assert_eq!(amounts, vec![100, 200]);
    }

    #[test]
    fn full_exit_returns_all() {
        let amounts = compute_proportional_exit(&[500, 1000, 250], 100, 100).unwrap();
        assert_eq!(amounts, vec![500, 1000, 250]);
    }
}
