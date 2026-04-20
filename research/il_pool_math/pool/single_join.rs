//! Single-asset join computation (Recipe 3.1.2).
//!
//! LP minted = supply * ((1 + fee_adj_in / B_i)^w_i - 1)
//! Uses Balancer partial fee: (1 - w_i) * swap_fee_rate.

use halcyon_common::constants::SCALE_U128;
use halcyon_common::fees::compute_fee_adj_in;
use halcyon_common::fp::{fp_to_token_floor, token_to_fp};
use solmath_core::SolMathError;

/// LP mint decimals (always 9).
const LP_DECIMALS: u8 = 9;

/// Compute LP output for a single-asset join.
///
/// # Arguments
/// * `balance_i` - Pool balance of the deposited token (raw)
/// * `weight_i` - Weight of the deposited token (at SCALE)
/// * `amount_in` - Amount deposited (raw, before partial fee)
/// * `swap_fee_rate` - Pool swap fee rate (at SCALE)
/// * `supply` - Current LP supply (raw, 9 decimals)
/// * `decimals_i` - Decimals of the deposited token (unused — ratio cancels decimals)
///
/// # Returns
/// `(fee_adj_in, lp_out)` — fee-adjusted input and LP minted (both raw).
pub fn compute_single_asset_join(
    balance_i: u64,
    weight_i: u64,
    amount_in: u64,
    swap_fee_rate: u64,
    supply: u64,
    _decimals_i: u8,
) -> Result<(u64, u64), SolMathError> {
    // Guard: zero amount_in — match on-chain require!(amount_in > 0).
    if amount_in == 0 {
        return Err(SolMathError::DomainError);
    }

    // Guard: balance must be positive (prevents division by zero in ratio step).
    if balance_i == 0 || supply == 0 {
        return Err(SolMathError::DomainError);
    }

    // Step 0: Balancer partial fee → fee_adj_in
    let fee_adj_in = compute_fee_adj_in(amount_in, weight_i, swap_fee_rate)?;

    // Step 1: ratio = fee_adj_in / B_i at SCALE
    // Same-token ratio — decimals cancel.
    let ratio = solmath_core::fp_div(fee_adj_in as u128, balance_i as u128)?;

    // Step 2: base = 1 + fee_adj_in / B_i
    let base = SCALE_U128 + ratio;

    // Step 3: pow_result = (1 + fee_adj_in / B_i)^w_i
    let pow_result = solmath_core::pow_fixed_hp(base, weight_i as u128)?;

    // Step 4: delta = pow_result - 1
    // For a join, base > SCALE so pow_result > SCALE. If not, inputs are invalid.
    let delta = pow_result
        .checked_sub(SCALE_U128)
        .ok_or(SolMathError::Overflow)?;

    // Step 5: supply_fp = LP supply at SCALE (supply × 10^(12-9) = supply × 1000)
    let supply_fp = token_to_fp(supply, LP_DECIMALS)?;

    // Step 6: lp_fp = supply × (pow_result - 1) at SCALE
    let lp_fp = solmath_core::fp_mul(supply_fp, delta)?;

    // Step 7: Convert back to LP raw units, floor (user gets less LP — pool-favourable)
    let lp_out = fp_to_token_floor(lp_fp, LP_DECIMALS)?;

    Ok((fee_adj_in, lp_out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_amount_in_rejected() {
        let err = compute_single_asset_join(
            1_000_000,
            500_000_000_000,
            0, // zero amount_in
            3_000_000_000,
            100_000_000_000,
            6,
        );
        assert!(err.is_err());
    }

    #[test]
    fn single_join_basic() {
        // 50/50 pool, balance 1M, deposit 100k, 0.3% fee, 6 decimals
        let (fee_adj, lp) = compute_single_asset_join(
            1_000_000,
            500_000_000_000, // 50%
            100_000,
            3_000_000_000,   // 0.3%
            100_000_000_000, // 100 * 1e9 LP
            6,
        )
        .unwrap();
        // Partial fee = (1 - 0.5) * 0.3% = 0.15% of amount_in
        // fee_adj_in = 100_000 - floor(100_000 * 0.15% ) ≈ 99_850
        assert!(fee_adj < 100_000);
        assert!(fee_adj > 99_000);
        assert!(lp > 0);
    }
}
