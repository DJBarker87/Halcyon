use solmath_core::SolMathError;

use crate::insurance::european_nig::nig_european_il_premium;

pub const CURRENT_ENGINE_VERSION: u16 = 1;
pub const TENOR_DAYS: u32 = 30;
pub const DEDUCTIBLE_S6: i64 = 10_000;
pub const CAP_S6: i64 = 70_000;
pub const MAX_PAYOUT_FRACTION_S6: i64 = CAP_S6 - DEDUCTIBLE_S6;
pub const DEDUCTIBLE_S12: u64 = 10_000_000_000;
pub const CAP_S12: u64 = 70_000_000_000;
pub const UNDERWRITING_LOAD_S6: i64 = 1_100_000;
pub const SIGMA_FLOOR_ANNUALISED_S6: i64 = 400_000;
pub const SIGMA_MULTIPLIER_CALM_S6: i64 = 1_300_000;
pub const SIGMA_MULTIPLIER_STRESS_S6: i64 = 2_000_000;
pub const FVOL_STRESS_THRESHOLD_S6: i64 = 600_000;
pub const NIG_ALPHA_S6: i64 = 3_140_100;
pub const NIG_BETA_S6: i64 = 1_213_900;
pub const POOL_WEIGHT_S12: u64 = 500_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegimeKind {
    Calm = 0,
    Stress = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegimeConfig {
    pub fvol_s6: i64,
    pub regime: RegimeKind,
    pub sigma_multiplier_s6: i64,
    pub sigma_floor_annualised_s6: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IlProtectionQuote {
    pub fair_premium_fraction_s6: i64,
    pub loaded_premium_fraction_s6: i64,
    pub premium_usdc: u64,
    pub max_liability_usdc: u64,
}

pub fn classify_regime_from_fvol_s6(fvol_s6: i64) -> RegimeConfig {
    if fvol_s6 >= FVOL_STRESS_THRESHOLD_S6 {
        RegimeConfig {
            fvol_s6,
            regime: RegimeKind::Stress,
            sigma_multiplier_s6: SIGMA_MULTIPLIER_STRESS_S6,
            sigma_floor_annualised_s6: SIGMA_FLOOR_ANNUALISED_S6,
        }
    } else {
        RegimeConfig {
            fvol_s6,
            regime: RegimeKind::Calm,
            sigma_multiplier_s6: SIGMA_MULTIPLIER_CALM_S6,
            sigma_floor_annualised_s6: SIGMA_FLOOR_ANNUALISED_S6,
        }
    }
}

pub fn price_il_protection(
    insured_notional_usdc: u64,
    sigma_ann_s6: i64,
) -> Result<IlProtectionQuote, SolMathError> {
    let fair_premium_fraction_s6 = nig_european_il_premium(
        sigma_ann_s6.max(SIGMA_FLOOR_ANNUALISED_S6),
        TENOR_DAYS,
        DEDUCTIBLE_S6,
        CAP_S6,
        NIG_ALPHA_S6,
        NIG_BETA_S6,
    )?;
    let loaded_premium_fraction_s6 = ceil_mul_s6(fair_premium_fraction_s6, UNDERWRITING_LOAD_S6)?;

    Ok(IlProtectionQuote {
        fair_premium_fraction_s6,
        loaded_premium_fraction_s6,
        premium_usdc: ceil_amount_mul_s6(insured_notional_usdc, loaded_premium_fraction_s6)?,
        max_liability_usdc: floor_amount_mul_s6(insured_notional_usdc, MAX_PAYOUT_FRACTION_S6)?,
    })
}

pub fn compute_fvol_from_daily_closes(prices: &[f64]) -> Option<f64> {
    compute_fvol_from_daily_closes_with_windows(prices, 7, 30)
}

fn compute_fvol_from_daily_closes_with_windows(
    prices: &[f64],
    vol_window: usize,
    fvol_window: usize,
) -> Option<f64> {
    if prices.len() < vol_window + fvol_window + 1 {
        return None;
    }

    let returns = prices
        .windows(2)
        .map(|pair| {
            let prev = *pair.first()?;
            let next = *pair.get(1)?;
            if !(prev.is_finite() && next.is_finite() && prev > 0.0 && next > 0.0) {
                return None;
            }
            Some((next / prev).ln())
        })
        .collect::<Option<Vec<_>>>()?;

    let annualiser = 365.0_f64.sqrt();
    let rolling_vols = returns
        .windows(vol_window)
        .map(|window| stddev(window).map(|sigma| sigma * annualiser))
        .collect::<Option<Vec<_>>>()?;
    let tail = rolling_vols.len().saturating_sub(fvol_window);
    stddev(&rolling_vols[tail..])
}

fn stddev(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let mean = values.iter().copied().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|value| {
            let delta = *value - mean;
            delta * delta
        })
        .sum::<f64>()
        / values.len() as f64;
    Some(variance.sqrt())
}

fn ceil_mul_s6(lhs_s6: i64, rhs_s6: i64) -> Result<i64, SolMathError> {
    let numerator = (lhs_s6 as i128)
        .checked_mul(rhs_s6 as i128)
        .ok_or(SolMathError::Overflow)?;
    let scaled = (numerator + 999_999i128)
        .checked_div(1_000_000i128)
        .ok_or(SolMathError::DivisionByZero)?;
    i64::try_from(scaled).map_err(|_| SolMathError::Overflow)
}

fn ceil_amount_mul_s6(amount: u64, fraction_s6: i64) -> Result<u64, SolMathError> {
    if fraction_s6 < 0 {
        return Err(SolMathError::DomainError);
    }
    let numerator = (amount as u128)
        .checked_mul(fraction_s6 as u128)
        .ok_or(SolMathError::Overflow)?;
    Ok(((numerator + 999_999u128) / 1_000_000u128) as u64)
}

fn floor_amount_mul_s6(amount: u64, fraction_s6: i64) -> Result<u64, SolMathError> {
    if fraction_s6 < 0 {
        return Err(SolMathError::DomainError);
    }
    Ok(((amount as u128)
        .checked_mul(fraction_s6 as u128)
        .ok_or(SolMathError::Overflow)?
        / 1_000_000u128) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calm_and_stress_regimes_map_to_expected_multipliers() {
        let calm = classify_regime_from_fvol_s6(590_000);
        assert_eq!(calm.regime, RegimeKind::Calm);
        assert_eq!(calm.sigma_multiplier_s6, SIGMA_MULTIPLIER_CALM_S6);

        let stress = classify_regime_from_fvol_s6(600_000);
        assert_eq!(stress.regime, RegimeKind::Stress);
        assert_eq!(stress.sigma_multiplier_s6, SIGMA_MULTIPLIER_STRESS_S6);
    }

    #[test]
    fn product_quote_has_loaded_premium_and_capped_liability() {
        let quote = price_il_protection(10_000_000_000, 800_000).expect("quote");
        assert!(quote.fair_premium_fraction_s6 > 0);
        assert!(quote.loaded_premium_fraction_s6 > quote.fair_premium_fraction_s6);
        assert_eq!(quote.max_liability_usdc, 600_000_000);
        assert!(quote.premium_usdc > 0);
    }

    #[test]
    fn fvol_requires_enough_history() {
        assert!(compute_fvol_from_daily_closes(&[100.0, 101.0]).is_none());
    }

    #[test]
    fn fvol_detects_choppier_series() {
        let calm = (0..60)
            .map(|i| 100.0 + (i as f64 * 0.25))
            .collect::<Vec<_>>();
        let stress = (0..60)
            .map(|i| {
                let wiggle = if i % 2 == 0 { 12.0 } else { -10.0 };
                100.0 + i as f64 * 0.5 + wiggle
            })
            .collect::<Vec<_>>();
        let calm_fvol = compute_fvol_from_daily_closes(&calm).expect("calm fvol");
        let stress_fvol = compute_fvol_from_daily_closes(&stress).expect("stress fvol");
        assert!(stress_fvol > calm_fvol);
    }
}
