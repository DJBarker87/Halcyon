use anchor_lang::prelude::*;
use halcyon_common::HalcyonError;
use halcyon_il_quote::{price_il_protection, IlProtectionQuote, RegimeKind};
use halcyon_kernel::state::{ProtocolConfig, RegimeSignal, VaultSigma};
use solana_sha256_hasher::hash;
use solmath_core::{fp_mul, fp_sqrt, SCALE};

use crate::errors::IlProtectionError;
use crate::state::{IssuedRegime, CURRENT_ENGINE_VERSION, SECONDS_PER_DAY, TENOR_DAYS};

pub struct QuoteOutputs {
    pub premium: u64,
    pub max_liability: u64,
    pub fair_premium_fraction_s6: i64,
    pub loaded_premium_fraction_s6: i64,
    pub sigma_pricing_s6: i64,
    pub expiry_ts: i64,
    pub quote_slot: u64,
    pub engine_version: u16,
}

pub fn compose_pricing_sigma(
    vault_sigma: &VaultSigma,
    regime_signal: &RegimeSignal,
    sigma_floor_annualised_s6: i64,
) -> Result<i64> {
    let floor_s6 = sigma_floor_annualised_s6.max(regime_signal.sigma_floor_annualised_s6);
    require!(floor_s6 > 0, IlProtectionError::InvalidSigmaFloor);

    if vault_sigma.ewma_var_daily_s12 <= 0 {
        return Ok(floor_s6);
    }

    let annual_variance_s12 = fp_mul(
        vault_sigma.ewma_var_daily_s12 as u128,
        365u128.checked_mul(SCALE).ok_or(HalcyonError::Overflow)?,
    )
    .map_err(|_| error!(HalcyonError::Overflow))?;
    let sigma_annual_s12 =
        fp_sqrt(annual_variance_s12).map_err(|_| error!(HalcyonError::Overflow))? as i128;
    let sigma_regime_s12 = sigma_annual_s12
        .checked_mul(regime_signal.sigma_multiplier_s6.max(0) as i128)
        .and_then(|value| value.checked_div(1_000_000i128))
        .ok_or(HalcyonError::Overflow)?;
    let sigma_s6 = i64::try_from(
        sigma_regime_s12
            .checked_div(1_000_000)
            .ok_or(HalcyonError::Overflow)?,
    )
    .map_err(|_| error!(HalcyonError::Overflow))?;

    Ok(sigma_s6.max(floor_s6))
}

pub fn protocol_sigma_floor_annualised_s6(config: &ProtocolConfig) -> i64 {
    config.sigma_floor_for_product_s6(&crate::ID)
}

pub fn solve_quote(
    sigma_pricing_s6: i64,
    insured_notional_usdc: u64,
    issued_at: i64,
) -> Result<QuoteOutputs> {
    let quote: IlProtectionQuote = price_il_protection(insured_notional_usdc, sigma_pricing_s6)
        .map_err(|_| error!(IlProtectionError::QuoteComputationFailed))?;
    let expiry_ts = issued_at
        .checked_add(TENOR_DAYS as i64 * SECONDS_PER_DAY)
        .ok_or(HalcyonError::Overflow)?;

    Ok(QuoteOutputs {
        premium: quote.premium_usdc,
        max_liability: quote.max_liability_usdc,
        fair_premium_fraction_s6: quote.fair_premium_fraction_s6,
        loaded_premium_fraction_s6: quote.loaded_premium_fraction_s6,
        sigma_pricing_s6,
        expiry_ts,
        quote_slot: Clock::get()?.slot,
        engine_version: CURRENT_ENGINE_VERSION,
    })
}

pub fn require_quote_acceptance_bounds(
    quote: &QuoteOutputs,
    preview_quote_slot: u64,
    max_quote_slot_delta: u64,
    live_entry_sol_price_s6: i64,
    preview_entry_sol_price_s6: i64,
    live_entry_usdc_price_s6: i64,
    preview_entry_usdc_price_s6: i64,
    max_entry_price_deviation_bps: u16,
    preview_expiry_ts: i64,
    max_expiry_delta_secs: i64,
) -> Result<()> {
    require!(
        quote.quote_slot >= preview_quote_slot,
        HalcyonError::SlippageExceeded
    );
    require!(
        quote.quote_slot - preview_quote_slot <= max_quote_slot_delta,
        HalcyonError::SlippageExceeded
    );
    require!(max_expiry_delta_secs >= 0, HalcyonError::SlippageExceeded);
    require!(
        quote.expiry_ts >= preview_expiry_ts,
        HalcyonError::SlippageExceeded
    );
    let expiry_delta_secs = quote
        .expiry_ts
        .checked_sub(preview_expiry_ts)
        .ok_or(HalcyonError::Overflow)?;
    require!(
        expiry_delta_secs <= max_expiry_delta_secs,
        HalcyonError::SlippageExceeded
    );
    require_price_drift_within_bounds(
        live_entry_sol_price_s6,
        preview_entry_sol_price_s6,
        max_entry_price_deviation_bps,
    )?;
    require_price_drift_within_bounds(
        live_entry_usdc_price_s6,
        preview_entry_usdc_price_s6,
        max_entry_price_deviation_bps,
    )?;
    Ok(())
}

fn require_price_drift_within_bounds(
    live_price_s6: i64,
    preview_price_s6: i64,
    max_entry_price_deviation_bps: u16,
) -> Result<()> {
    require!(live_price_s6 > 0, IlProtectionError::InvalidEntryPrice);
    require!(preview_price_s6 > 0, IlProtectionError::InvalidEntryPrice);
    let price_delta = i128::from(live_price_s6)
        .checked_sub(i128::from(preview_price_s6))
        .ok_or(HalcyonError::Overflow)?
        .abs();
    let price_delta_bps = price_delta
        .checked_mul(10_000)
        .and_then(|value| value.checked_div(i128::from(preview_price_s6)))
        .ok_or(HalcyonError::Overflow)?;
    require!(
        price_delta_bps <= i128::from(max_entry_price_deviation_bps),
        HalcyonError::SlippageExceeded
    );
    Ok(())
}

pub fn hash_product_terms(terms: &crate::state::IlProtectionTerms) -> Result<[u8; 32]> {
    use anchor_lang::Discriminator;
    let mut buf = Vec::with_capacity(8 + crate::state::IlProtectionTerms::INIT_SPACE);
    buf.extend_from_slice(&crate::state::IlProtectionTerms::DISCRIMINATOR);
    terms
        .serialize(&mut buf)
        .map_err(|_| error!(HalcyonError::Overflow))?;
    Ok(hash(&buf).to_bytes())
}

pub fn require_sigma_fresh(vault_sigma: &VaultSigma, now: i64, cap_secs: i64) -> Result<()> {
    let age = now
        .checked_sub(vault_sigma.ewma_last_timestamp)
        .ok_or(HalcyonError::Overflow)?;
    require!(age <= cap_secs, HalcyonError::SigmaStale);
    Ok(())
}

pub fn require_regime_fresh(regime_signal: &RegimeSignal, now: i64, cap_secs: i64) -> Result<()> {
    let age = now
        .checked_sub(regime_signal.last_update_ts)
        .ok_or(HalcyonError::Overflow)?;
    require!(age <= cap_secs, HalcyonError::RegimeStale);
    Ok(())
}

pub fn require_protocol_unpaused(config: &ProtocolConfig) -> Result<()> {
    require!(!config.issuance_paused_global, HalcyonError::PausedGlobally);
    Ok(())
}

pub fn issued_regime(regime_signal: &RegimeSignal) -> Result<IssuedRegime> {
    match regime_signal.regime {
        halcyon_kernel::state::Regime::Calm => Ok(IssuedRegime::Calm),
        halcyon_kernel::state::Regime::Stress => Ok(IssuedRegime::Stress),
    }
}

pub fn regime_kind_tag(regime_signal: &RegimeSignal) -> u8 {
    match regime_signal.regime {
        halcyon_kernel::state::Regime::Calm => RegimeKind::Calm as u8,
        halcyon_kernel::state::Regime::Stress => RegimeKind::Stress as u8,
    }
}
