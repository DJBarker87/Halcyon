//! Mid-life valuation for SOL/USDC IL protection.
//!
//! `nav_s6` is the fair remaining protection value under the same NIG European
//! IL model used at issuance, shifted by the live pool price move. Lending value
//! remains deliberately conservative: it is advanced only against current
//! intrinsic settlement value, not future optionality.

use serde::{Deserialize, Serialize};
use solmath_core::{div6, ln6, SolMathError};

use crate::insurance::european_nig::nig_european_il_premium_shifted;
use crate::insurance::settlement::compute_settlement_from_prices;
use crate::{NIG_ALPHA_S6, NIG_BETA_S6, POOL_WEIGHT_S12};

pub const SCALE_S6: i64 = 1_000_000;
pub const SCALE_12_PER_SCALE_6: u128 = 1_000_000;
pub const IL_LENDING_ADVANCE_RATE_S6: i64 = 800_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IlProtectionMidlifeInputs {
    pub weight_s12: u64,
    pub entry_sol_price_s6: i64,
    pub entry_usdc_price_s6: i64,
    pub current_sol_price_s6: i64,
    pub current_usdc_price_s6: i64,
    pub insured_notional_usdc: u64,
    pub deductible_s6: i64,
    pub cap_s6: i64,
    pub sigma_annual_s6: i64,
    pub remaining_days: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IlProtectionMidlifeNav {
    pub nav_s6: i64,
    pub max_cover_s6: i64,
    pub lending_value_s6: i64,
    pub terminal_il_s6: i64,
    pub terminal_il_s12: u128,
    pub nav_payout_usdc: u64,
    pub intrinsic_payout_usdc: u64,
    pub lending_value_payout_usdc: u64,
    pub current_log_ratio_s6: i64,
    pub sigma_pricing_s6: i64,
    pub remaining_days: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IlProtectionMidlifeError {
    InvalidInput,
    Overflow,
    SettlementMath,
    Pricing,
}

pub fn price_midlife_nav(
    inputs: &IlProtectionMidlifeInputs,
) -> Result<IlProtectionMidlifeNav, IlProtectionMidlifeError> {
    validate_inputs(inputs)?;
    let deductible_s12 = s6_to_s12(inputs.deductible_s6)?;
    let cap_s12 = s6_to_s12(inputs.cap_s6)?;
    let (terminal_il_s12, intrinsic_payout_usdc) = compute_settlement_from_prices(
        inputs.weight_s12,
        price_s6_to_s12(inputs.current_sol_price_s6)?,
        price_s6_to_s12(inputs.current_usdc_price_s6)?,
        price_s6_to_s12(inputs.entry_sol_price_s6)?,
        price_s6_to_s12(inputs.entry_usdc_price_s6)?,
        inputs.insured_notional_usdc,
        deductible_s12 as u64,
        cap_s12 as u64,
    )
    .map_err(|_| IlProtectionMidlifeError::SettlementMath)?;

    let intrinsic_s6 = payout_to_fraction_s6(intrinsic_payout_usdc, inputs.insured_notional_usdc)?;
    let current_log_ratio_s6 = current_log_ratio_s6(inputs)?;
    let fair_nav_s6 = if inputs.remaining_days == 0 {
        intrinsic_s6
    } else {
        nig_european_il_premium_shifted(
            inputs.sigma_annual_s6,
            inputs.remaining_days,
            inputs.deductible_s6,
            inputs.cap_s6,
            NIG_ALPHA_S6,
            NIG_BETA_S6,
            current_log_ratio_s6,
        )
        .map_err(map_pricing_error)?
    };
    let nav_s6 = fair_nav_s6
        .max(intrinsic_s6)
        .min(inputs.cap_s6 - inputs.deductible_s6);
    let conservative_lending_value_s6 = mul_fraction_s6(intrinsic_s6, IL_LENDING_ADVANCE_RATE_S6)?;
    let terminal_il_s6 = i64::try_from(terminal_il_s12 / SCALE_12_PER_SCALE_6)
        .map_err(|_| IlProtectionMidlifeError::Overflow)?;

    Ok(IlProtectionMidlifeNav {
        nav_s6,
        max_cover_s6: inputs
            .cap_s6
            .checked_sub(inputs.deductible_s6)
            .ok_or(IlProtectionMidlifeError::Overflow)?,
        lending_value_s6: conservative_lending_value_s6,
        terminal_il_s6,
        terminal_il_s12,
        nav_payout_usdc: amount_mul_s6(inputs.insured_notional_usdc, nav_s6)?
            .max(intrinsic_payout_usdc),
        intrinsic_payout_usdc,
        lending_value_payout_usdc: amount_mul_s6(
            inputs.insured_notional_usdc,
            conservative_lending_value_s6,
        )?,
        current_log_ratio_s6,
        sigma_pricing_s6: inputs.sigma_annual_s6,
        remaining_days: inputs.remaining_days,
    })
}

fn validate_inputs(inputs: &IlProtectionMidlifeInputs) -> Result<(), IlProtectionMidlifeError> {
    if inputs.insured_notional_usdc == 0
        || inputs.entry_sol_price_s6 <= 0
        || inputs.entry_usdc_price_s6 <= 0
        || inputs.current_sol_price_s6 <= 0
        || inputs.current_usdc_price_s6 <= 0
        || inputs.deductible_s6 < 0
        || inputs.cap_s6 <= inputs.deductible_s6
        || inputs.sigma_annual_s6 <= 0
        || inputs.weight_s12 != POOL_WEIGHT_S12
    {
        return Err(IlProtectionMidlifeError::InvalidInput);
    }
    Ok(())
}

fn current_log_ratio_s6(
    inputs: &IlProtectionMidlifeInputs,
) -> Result<i64, IlProtectionMidlifeError> {
    let current_pair_ratio =
        div6(inputs.current_sol_price_s6, inputs.current_usdc_price_s6).map_err(map_math_error)?;
    let entry_pair_ratio =
        div6(inputs.entry_sol_price_s6, inputs.entry_usdc_price_s6).map_err(map_math_error)?;
    let relative_ratio = div6(current_pair_ratio, entry_pair_ratio).map_err(map_math_error)?;
    ln6(relative_ratio).map_err(map_math_error)
}

fn map_math_error(_err: SolMathError) -> IlProtectionMidlifeError {
    IlProtectionMidlifeError::Pricing
}

fn map_pricing_error(_err: SolMathError) -> IlProtectionMidlifeError {
    IlProtectionMidlifeError::Pricing
}

fn price_s6_to_s12(price_s6: i64) -> Result<u128, IlProtectionMidlifeError> {
    if price_s6 <= 0 {
        return Err(IlProtectionMidlifeError::InvalidInput);
    }
    (price_s6 as u128)
        .checked_mul(SCALE_12_PER_SCALE_6)
        .ok_or(IlProtectionMidlifeError::Overflow)
}

fn s6_to_s12(value_s6: i64) -> Result<u128, IlProtectionMidlifeError> {
    if value_s6 < 0 {
        return Err(IlProtectionMidlifeError::InvalidInput);
    }
    (value_s6 as u128)
        .checked_mul(SCALE_12_PER_SCALE_6)
        .ok_or(IlProtectionMidlifeError::Overflow)
}

fn payout_to_fraction_s6(
    payout_usdc: u64,
    notional_usdc: u64,
) -> Result<i64, IlProtectionMidlifeError> {
    if notional_usdc == 0 {
        return Err(IlProtectionMidlifeError::InvalidInput);
    }
    let value = (payout_usdc as u128)
        .checked_mul(SCALE_S6 as u128)
        .and_then(|value| value.checked_div(notional_usdc as u128))
        .ok_or(IlProtectionMidlifeError::Overflow)?;
    i64::try_from(value).map_err(|_| IlProtectionMidlifeError::Overflow)
}

fn mul_fraction_s6(lhs_s6: i64, rhs_s6: i64) -> Result<i64, IlProtectionMidlifeError> {
    if lhs_s6 < 0 || rhs_s6 < 0 {
        return Err(IlProtectionMidlifeError::InvalidInput);
    }
    let value = i128::from(lhs_s6)
        .checked_mul(i128::from(rhs_s6))
        .and_then(|value| value.checked_div(i128::from(SCALE_S6)))
        .ok_or(IlProtectionMidlifeError::Overflow)?;
    i64::try_from(value).map_err(|_| IlProtectionMidlifeError::Overflow)
}

fn amount_mul_s6(amount: u64, fraction_s6: i64) -> Result<u64, IlProtectionMidlifeError> {
    if fraction_s6 <= 0 {
        return Ok(0);
    }
    let value = (amount as u128)
        .checked_mul(fraction_s6 as u128)
        .and_then(|value| value.checked_div(SCALE_S6 as u128))
        .ok_or(IlProtectionMidlifeError::Overflow)?;
    u64::try_from(value).map_err(|_| IlProtectionMidlifeError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CAP_S6, DEDUCTIBLE_S6, POOL_WEIGHT_S12};

    fn sample_inputs() -> IlProtectionMidlifeInputs {
        IlProtectionMidlifeInputs {
            weight_s12: POOL_WEIGHT_S12,
            entry_sol_price_s6: 150_000_000,
            entry_usdc_price_s6: 1_000_000,
            current_sol_price_s6: 150_000_000,
            current_usdc_price_s6: 1_000_000,
            insured_notional_usdc: 10_000_000_000,
            deductible_s6: DEDUCTIBLE_S6,
            cap_s6: CAP_S6,
            sigma_annual_s6: 800_000,
            remaining_days: 30,
        }
    }

    fn assert_conservative_advance(nav: &IlProtectionMidlifeNav) {
        let expected_floor = nav.intrinsic_payout_usdc * 80 / 100;
        assert!(nav.lending_value_payout_usdc <= expected_floor);
        assert!(nav.lending_value_payout_usdc + 25_000 >= expected_floor);
    }

    #[test]
    fn unchanged_pool_has_positive_fair_nav_but_zero_lending_value() {
        let nav = price_midlife_nav(&sample_inputs()).expect("nav");
        assert!(nav.nav_s6 > 0);
        assert_eq!(nav.intrinsic_payout_usdc, 0);
        assert_eq!(nav.lending_value_payout_usdc, 0);
    }

    #[test]
    fn price_move_creates_lendable_intrinsic_value() {
        let mut inputs = sample_inputs();
        inputs.current_sol_price_s6 = 300_000_000;
        let nav = price_midlife_nav(&inputs).expect("nav");
        assert!(nav.terminal_il_s6 > DEDUCTIBLE_S6);
        assert!(nav.intrinsic_payout_usdc > 0);
        assert_conservative_advance(&nav);
        assert!(nav.nav_payout_usdc >= nav.intrinsic_payout_usdc);
    }

    #[test]
    fn expiry_nav_equals_intrinsic_value() {
        let mut inputs = sample_inputs();
        inputs.current_sol_price_s6 = 300_000_000;
        inputs.remaining_days = 0;
        let nav = price_midlife_nav(&inputs).expect("nav");
        assert_eq!(nav.nav_payout_usdc, nav.intrinsic_payout_usdc);
    }

    #[test]
    fn no_lending_against_future_optionality_on_backtest_grid() {
        let mut inputs = sample_inputs();
        for price in [
            75_000_000,
            100_000_000,
            125_000_000,
            150_000_000,
            200_000_000,
            300_000_000,
        ] {
            inputs.current_sol_price_s6 = price;
            let nav = price_midlife_nav(&inputs).expect("nav");
            assert!(nav.nav_s6 <= CAP_S6 - DEDUCTIBLE_S6);
            assert_conservative_advance(&nav);
        }
    }
}
