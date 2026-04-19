//! EWMA variance update logic (pure math).
//!
//! Computes the exponentially weighted moving average of log returns
//! for use in premium computation. This is the stateless math layer —
//! the on-chain instruction in halcyon-insurance handles state management,
//! oracle reads, and guard rails.
//!
//! Audit fix M-5: aligned with on-chain convention.
//! - `update_ewma_variance` now normalises r² by Δt to produce daily variance.
//! - `annualise_variance` uses √(daily_var × 365) only — no observations_per_day factor.

use solmath_core::{
    exp_fixed_i, fp_div_i, fp_mul, fp_mul_i, fp_sqrt, ln_fixed_i, SolMathError, SCALE, SCALE_I,
};

/// Update EWMA variance with a new price observation.
///
/// variance_new = λ × variance_old + (1 - λ) × (r² / Δt_days)
///
/// where r = ln(price_new / price_old), Δt_days = dt_seconds / 86400,
/// and λ = exp(-Δt / τ).
///
/// Returns updated EWMA **daily** variance at SCALE.
///
/// # Arguments
/// * `prev_variance` - Previous EWMA daily variance (at SCALE, u128)
/// * `prev_price` - Previous oracle price (at SCALE, i128, must be > 0)
/// * `current_price` - Current oracle price (at SCALE, i128, must be > 0)
/// * `tau_seconds` - EWMA time constant in seconds (e.g. 604800 for ~7 days)
/// * `dt_seconds` - Time elapsed since last observation in seconds
pub fn update_ewma_variance(
    prev_variance: u128,
    prev_price: i128,
    current_price: i128,
    tau_seconds: u64,
    dt_seconds: u64,
) -> Result<u128, SolMathError> {
    if prev_price <= 0 || current_price <= 0 {
        return Err(SolMathError::DomainError);
    }
    if tau_seconds == 0 || dt_seconds == 0 {
        return Ok(prev_variance);
    }

    // r = ln(price_new / price_old)
    let ratio = fp_div_i(current_price, prev_price)?;
    if ratio <= 0 {
        return Err(SolMathError::DomainError);
    }
    let r = ln_fixed_i(ratio as u128)?;

    // r² at SCALE
    let r_sq = fp_mul_i(r, r)?;
    let r_sq_u = if r_sq < 0 { 0u128 } else { r_sq as u128 };

    // Δt in fractional days at SCALE: dt_days = dt_seconds × SCALE / 86400
    let dt_days_i = (dt_seconds as i128)
        .checked_mul(SCALE_I)
        .ok_or(SolMathError::Overflow)?
        / 86400;

    if dt_days_i <= 0 {
        return Ok(prev_variance);
    }

    // instant_var = r² / Δt_days (daily variance for this observation)
    let instant_var_i = fp_div_i(r_sq as i128, dt_days_i)?;
    let instant_var = if instant_var_i < 0 {
        0u128
    } else {
        instant_var_i as u128
    };

    // λ = exp(-Δt / τ)
    // -Δt/τ at SCALE
    let neg_dt_over_tau = -(SCALE_I * (dt_seconds as i128)) / (tau_seconds as i128);
    let lambda = exp_fixed_i(neg_dt_over_tau)?;
    let lambda_u = lambda.max(0) as u128;

    // variance_new = λ × prev + (1-λ) × instant_var
    let one_minus_lambda = SCALE.saturating_sub(lambda_u);
    let term1 = fp_mul(lambda_u, prev_variance)?;
    let term2 = fp_mul(one_minus_lambda, instant_var)?;

    Ok(term1 + term2)
}

/// Annualise a daily EWMA variance to annualised volatility.
///
/// σ_annual = √(daily_variance × 365)
///
/// Returns annualised sigma at SCALE.
pub fn annualise_variance(daily_variance: u128) -> Result<u128, SolMathError> {
    if daily_variance == 0 {
        return Ok(0);
    }

    // annual_variance = daily_variance × 365
    let annual_var = fp_mul(daily_variance, 365 * SCALE)?;

    // sigma = sqrt(annual_var)
    let sigma = fp_sqrt(annual_var)?;

    Ok(sigma as u128)
}

/// Blend standard and long EWMA sigmas based on term.
///
/// Selection at pricing time (per redesign doc):
///   ≤30 days: σ = sigma_standard
///   ≥90 days: σ = sigma_long (theoretical, but we blend for 31-90)
///   31-89 days: linear blend
///
/// Returns blended sigma at SCALE.
pub fn blend_sigma(sigma_standard: u64, sigma_long: u64, term_days: u16) -> u64 {
    if term_days <= 30 {
        sigma_standard
    } else if term_days >= 90 {
        sigma_long
    } else {
        // Linear blend: alpha = (term - 30) / 60
        let alpha_num = (term_days - 30) as u64;
        let alpha_den = 60u64;
        let blended = sigma_standard as u128 * (alpha_den - alpha_num) as u128
            + sigma_long as u128 * alpha_num as u128;
        (blended / alpha_den as u128) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCALE_U128: u128 = SCALE;

    #[test]
    fn ewma_no_price_change() {
        let prev_var = 100_000_000_000u128; // some variance
        let price = 100 * SCALE_I; // $100

        let new_var = update_ewma_variance(prev_var, price, price, 604800, 3600).unwrap();

        // No price change → r=0, instant_var=0 → variance decays toward 0
        assert!(
            new_var < prev_var,
            "variance should decay with no price change"
        );
        assert!(new_var > 0, "variance should not go to zero in one step");
    }

    #[test]
    fn ewma_price_increase() {
        let prev_var = 1_000_000u128; // very small
        let prev_price = 100 * SCALE_I;
        let new_price = 105 * SCALE_I; // 5% up

        let new_var = update_ewma_variance(prev_var, prev_price, new_price, 604800, 3600).unwrap();

        assert!(
            new_var > prev_var,
            "5% move should increase low variance, got {new_var}"
        );
    }

    #[test]
    fn ewma_price_decrease() {
        let prev_var = 1_000_000u128;
        let prev_price = 100 * SCALE_I;
        let new_price = 95 * SCALE_I; // 5% down

        let new_var = update_ewma_variance(prev_var, prev_price, new_price, 604800, 3600).unwrap();

        assert!(
            new_var > prev_var,
            "5% move should increase low variance, got {new_var}"
        );
    }

    #[test]
    fn ewma_invalid_prices() {
        assert!(update_ewma_variance(0, 0, SCALE_I, 604800, 3600).is_err());
        assert!(update_ewma_variance(0, SCALE_I, 0, 604800, 3600).is_err());
        assert!(update_ewma_variance(0, -SCALE_I, SCALE_I, 604800, 3600).is_err());
    }

    #[test]
    fn annualise_zero() {
        assert_eq!(annualise_variance(0).unwrap(), 0);
    }

    #[test]
    fn annualise_reasonable() {
        // Daily variance of (0.01)² = 0.0001 at SCALE
        // σ = sqrt(0.0001 × 365) ≈ sqrt(0.0365) ≈ 0.191
        // At SCALE: ~191_000_000_000
        let daily_var = 100_000_000u128; // 0.0001 at SCALE
        let sigma = annualise_variance(daily_var).unwrap();

        assert!(sigma > 180_000_000_000, "sigma={sigma}, expected > 0.18");
        assert!(sigma < 200_000_000_000, "sigma={sigma}, expected < 0.20");
    }

    #[test]
    fn annualise_80pct_daily_var() {
        // σ_annual = 0.80 → daily_var = 0.80² / 365 ≈ 0.001753
        // At SCALE: 1_753_424_658
        let daily_var = 1_753_424_658u128;
        let sigma = annualise_variance(daily_var).unwrap();

        let diff = if sigma > 800_000_000_000 {
            sigma - 800_000_000_000
        } else {
            800_000_000_000 - sigma
        };
        assert!(
            diff < 500_000_000,
            "annualised σ = {sigma}, expected ~800B, diff = {diff}",
        );
    }

    #[test]
    fn blend_at_boundaries() {
        assert_eq!(
            blend_sigma(400_000_000_000, 700_000_000_000, 30),
            400_000_000_000
        );
        assert_eq!(
            blend_sigma(400_000_000_000, 700_000_000_000, 90),
            700_000_000_000
        );
    }

    #[test]
    fn blend_midpoint() {
        let blended = blend_sigma(400_000_000_000, 700_000_000_000, 60);
        // At day 60: alpha = (60-30)/60 = 0.5 → blend = 550_000_000_000
        assert_eq!(blended, 550_000_000_000);
    }

    #[test]
    fn blend_short_term() {
        assert_eq!(
            blend_sigma(400_000_000_000, 700_000_000_000, 7),
            400_000_000_000
        );
        assert_eq!(
            blend_sigma(400_000_000_000, 700_000_000_000, 1),
            400_000_000_000
        );
    }
}
