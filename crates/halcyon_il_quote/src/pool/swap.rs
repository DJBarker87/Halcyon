//! Swap output computation (Recipe 3.1.1).
//!
//! Implements the weighted pool swap formula with fee-on-input:
//!   effective_in = amount_in - fee
//!   amount_out = B_j * (1 - (B_i / (B_i + effective_in))^(w_i/w_j))

use halcyon_common::constants::SCALE_U128;
use halcyon_common::fees::compute_effective_in;
use halcyon_common::fp::{fp_to_token_floor, token_to_fp};
use solmath_core::SolMathError;

/// Compute swap output for a weighted pool.
///
/// # Arguments
/// * `balance_in` - Pool balance of input token (raw)
/// * `balance_out` - Pool balance of output token (raw)
/// * `weight_in` - Weight of input token (at SCALE)
/// * `weight_out` - Weight of output token (at SCALE)
/// * `amount_in` - Amount of input token (raw, before fee)
/// * `swap_fee_rate` - Fee rate (at SCALE)
/// * `decimals_in` - Decimals of input token (unused — decimals cancel in ratio)
/// * `decimals_out` - Decimals of output token
///
/// # Returns
/// `(effective_in, amount_out)` — both in raw token units.
pub fn compute_swap_output(
    balance_in: u64,
    balance_out: u64,
    weight_in: u64,
    weight_out: u64,
    amount_in: u64,
    swap_fee_rate: u64,
    _decimals_in: u8,
    decimals_out: u8,
) -> Result<(u64, u64), SolMathError> {
    // Guard: zero amount_in produces no output — match on-chain require!(amount_in > 0).
    if amount_in == 0 {
        return Err(SolMathError::DomainError);
    }

    // Guard: balances must be positive (on-chain checks this; replicate here
    // so off-chain quoters get a clear error instead of nonsensical results).
    if balance_in == 0 || balance_out == 0 {
        return Err(SolMathError::DomainError);
    }

    // Step 0: Fee-on-input
    let effective_in = compute_effective_in(amount_in, swap_fee_rate)?;

    // Step 1: base = B_i / (B_i + effective_in) at SCALE
    // Same-token ratio — decimals cancel, so use raw u128 values.
    let b_i = balance_in as u128;
    let eff = effective_in as u128;
    let base = solmath_core::fp_div(b_i, b_i + eff)?;

    // Step 2: exponent = w_i / w_j at SCALE
    let w_i = weight_in as u128;
    let w_j = weight_out as u128;
    let exponent = solmath_core::fp_div(w_i, w_j)?;

    // Step 3: pow_result = (B_i / (B_i + effective_in))^(w_i/w_j)
    let pow_result = solmath_core::pow_fixed_hp(base, exponent)?;

    // Step 4: delta = 1 - pow_result
    // checked_sub: pow_result > SCALE indicates a math error (should not happen
    // for valid inputs). Fail explicitly rather than silently clamping to 0.
    let delta = SCALE_U128
        .checked_sub(pow_result)
        .ok_or(SolMathError::Overflow)?;

    // Step 5: B_j_fp = balance_out at SCALE (normalised by decimals)
    let b_j_fp = token_to_fp(balance_out, decimals_out)?;

    // Step 6: gross_out_fp = B_j × delta at SCALE
    let gross_out_fp = solmath_core::fp_mul(b_j_fp, delta)?;

    // Step 7: Convert back to raw, floor (favours pool)
    let amount_out = fp_to_token_floor(gross_out_fp, decimals_out)?;

    Ok((effective_in, amount_out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_weight_swap() {
        // 50/50 pool, 1M balance each, 0.3% fee, 6 decimals
        let (eff, out) = compute_swap_output(
            1_000_000,
            1_000_000,
            500_000_000_000,
            500_000_000_000,
            100_000,
            3_000_000_000,
            6,
            6,
        )
        .unwrap();
        // effective_in = 100_000 - floor(100_000 * 3e9 / 1e12) = 100_000 - 300 = 99_700
        assert_eq!(eff, 99_700);
        // For equal weights, exponent = 1, pow is exact: base = 1M/(1M+99700)
        // amount_out should be close to 99_700 * 1M / (1M + 99_700) ≈ 90_796
        assert!(out > 0);
        assert!(out < 100_000);
    }

    #[test]
    fn zero_amount_in_rejected() {
        let err = compute_swap_output(
            1_000_000,
            1_000_000,
            500_000_000_000,
            500_000_000_000,
            0,
            3_000_000_000,
            6,
            6,
        );
        assert!(err.is_err());
    }

    #[test]
    fn zero_fee_swap() {
        // With MIN_SWAP_FEE (0.01%) on a large balance
        let (eff, out) = compute_swap_output(
            1_000_000_000,
            1_000_000_000,
            500_000_000_000,
            500_000_000_000,
            10_000_000,
            100_000_000, // 0.01% fee
            9,
            9,
        )
        .unwrap();
        assert_eq!(eff, 10_000_000 - 1_000); // fee = floor(10M * 1e8 / 1e12) = 1000
        assert!(out > 0);
    }
}
