//! Single-asset exit computation (Recipe 3.1.3).
//!
//! gross_out = B_i * (1 - (1 - burn / supply)^(1/w_i))
//! net_out = gross_out - partial_fee(gross_out)

use halcyon_common::constants::SCALE_U128;
use halcyon_common::fees::compute_net_out;
use halcyon_common::fp::{fp_to_token_floor, token_to_fp};
use solmath_core::SolMathError;

/// Compute token output for a single-asset exit.
///
/// # Arguments
/// * `balance_i` - Pool balance of the withdrawn token (raw)
/// * `weight_i` - Weight of the withdrawn token (at SCALE)
/// * `burn_amount` - LP tokens to burn (raw, 9 decimals)
/// * `swap_fee_rate` - Pool swap fee rate (at SCALE)
/// * `supply` - Current LP supply before burn (raw, 9 decimals)
/// * `decimals_i` - Decimals of the withdrawn token
///
/// # Returns
/// `(gross_out, net_out)` — before and after partial fee (both raw).
///
/// Precondition: `burn_amount < supply` (strict less-than).
pub fn compute_single_asset_exit(
    balance_i: u64,
    weight_i: u64,
    burn_amount: u64,
    swap_fee_rate: u64,
    supply: u64,
    decimals_i: u8,
) -> Result<(u64, u64), SolMathError> {
    // Guard: zero burn_amount — match on-chain require!(burn_amount > 0).
    if burn_amount == 0 {
        return Err(SolMathError::DomainError);
    }

    // Enforce documented precondition: burn < supply (strict).
    // burn == supply must use proportional_exit (which handles dead-pool transition).
    // burn > supply is nonsensical. Without this guard, burn >= supply drains
    // 100% of one token, leaving other token balances stranded with no LP to claim them.
    if burn_amount >= supply || supply == 0 {
        return Err(SolMathError::DomainError);
    }

    // Step 1: ratio = burn / supply at SCALE
    // Both are 9-decimal LP values — decimals cancel.
    let ratio = solmath_core::fp_div(burn_amount as u128, supply as u128)?;

    // Step 2: base = 1 - burn/supply
    // Safe: burn < supply (enforced above) guarantees ratio < SCALE.
    let base = SCALE_U128
        .checked_sub(ratio)
        .ok_or(SolMathError::Overflow)?;

    // Step 3: inv_w = 1 / w_i at SCALE
    let inv_w = solmath_core::fp_div(SCALE_U128, weight_i as u128)?;

    // Step 4: pow_result = (1 - burn/supply)^(1/w_i)
    let pow_result = solmath_core::pow_fixed_hp(base, inv_w)?;

    // Step 5: delta = 1 - pow_result
    let delta = SCALE_U128
        .checked_sub(pow_result)
        .ok_or(SolMathError::Overflow)?;

    // Step 6: B_i_fp = balance at SCALE (normalised by decimals)
    let b_i_fp = token_to_fp(balance_i, decimals_i)?;

    // Step 7: gross_fp = B_i × (1 - pow_result) at SCALE
    let gross_fp = solmath_core::fp_mul(b_i_fp, delta)?;

    // Step 8: Convert back to raw, floor (favours pool)
    let gross_out = fp_to_token_floor(gross_fp, decimals_i)?;

    // Step 9: Apply Balancer partial fee to get net_out
    let net_out = compute_net_out(gross_out, weight_i, swap_fee_rate)?;

    Ok((gross_out, net_out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_burn_rejected() {
        let err = compute_single_asset_exit(
            1_000_000,
            500_000_000_000,
            0, // zero burn
            3_000_000_000,
            100_000_000_000,
            6,
        );
        assert!(err.is_err());
    }

    #[test]
    fn single_exit_basic() {
        // 50/50 pool, balance 1M, burn 10% of supply, 0.3% fee
        let (gross, net) = compute_single_asset_exit(
            1_000_000,
            500_000_000_000, // 50%
            10_000_000_000,  // 10B LP (10% of 100B)
            3_000_000_000,   // 0.3%
            100_000_000_000, // 100B LP
            6,
        )
        .unwrap();
        assert!(gross > 0);
        assert!(net > 0);
        assert!(net <= gross); // fee reduces output
        assert!(gross < 1_000_000); // can't drain pool
    }

    #[test]
    fn single_exit_net_less_than_gross() {
        let (gross, net) = compute_single_asset_exit(
            10_000_000,
            300_000_000_000, // 30%
            5_000_000_000,
            10_000_000_000, // 1% fee
            100_000_000_000,
            6,
        )
        .unwrap();
        assert!(net < gross);
    }
}
