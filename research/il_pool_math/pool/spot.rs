//! Spot price computation.
//!
//! spot_price = (B_i_fp / w_i) / (B_j_fp / w_j)
//!
//! Result is in UI-per-UI units: how many UI units of token_j per one UI unit of token_i.
//! token_to_fp normalises both sides to SCALE, so decimal differences cancel.

use halcyon_common::fp::token_to_fp;
use solmath_core::SolMathError;

/// Compute the spot price of token_j in terms of token_i.
///
/// Returns the price at SCALE (fixed-point u128).
pub fn compute_spot_price(
    balance_i: u64,
    balance_j: u64,
    weight_i: u64,
    weight_j: u64,
    decimals_i: u8,
    decimals_j: u8,
) -> Result<u128, SolMathError> {
    // Normalise balances to SCALE
    let b_i_fp = token_to_fp(balance_i, decimals_i)?;
    let b_j_fp = token_to_fp(balance_j, decimals_j)?;

    // numerator = B_i_fp / w_i
    let numerator = solmath_core::fp_div(b_i_fp, weight_i as u128)?;

    // denominator = B_j_fp / w_j
    let denominator = solmath_core::fp_div(b_j_fp, weight_j as u128)?;

    // spot = numerator / denominator
    solmath_core::fp_div(numerator, denominator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use halcyon_common::constants::SCALE;

    #[test]
    fn equal_weight_equal_balance_price_is_one() {
        let price =
            compute_spot_price(1_000_000, 1_000_000, 500_000_000_000, 500_000_000_000, 6, 6)
                .unwrap();
        assert_eq!(price, SCALE as u128);
    }

    #[test]
    fn double_balance_doubles_price() {
        let price =
            compute_spot_price(2_000_000, 1_000_000, 500_000_000_000, 500_000_000_000, 6, 6)
                .unwrap();
        assert_eq!(price, 2 * SCALE as u128);
    }

    #[test]
    fn weight_asymmetry_affects_price() {
        // 80/20 pool with equal balances
        let price =
            compute_spot_price(1_000_000, 1_000_000, 800_000_000_000, 200_000_000_000, 6, 6)
                .unwrap();
        // (B_i/w_i) / (B_j/w_j) = (1/0.8) / (1/0.2) = 0.25
        assert_eq!(price, SCALE as u128 / 4);
    }
}
