//! `preview_lending_value` - read-only IL protection midlife collateral mark.

use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};
use halcyon_il_quote::midlife::{
    price_midlife_nav, IlProtectionMidlifeError, IlProtectionMidlifeInputs,
};
use halcyon_kernel::state::{PolicyHeader, PolicyStatus, ProtocolConfig, RegimeSignal, VaultSigma};
use halcyon_kernel::KernelError;

use crate::errors::IlProtectionError;
use crate::pricing::{
    compose_pricing_sigma, protocol_sigma_floor_annualised_s6, require_regime_fresh,
    require_sigma_fresh,
};
use crate::state::{IlProtectionTerms, ProductStatus, CURRENT_ENGINE_VERSION, SECONDS_PER_DAY};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct LendingValuePreview {
    pub nav_s6: i64,
    pub max_cover_s6: i64,
    pub lending_value_s6: i64,
    pub nav_payout_usdc: u64,
    pub lending_value_payout_usdc: u64,
    pub terminal_il_s6: i64,
    pub terminal_il_s12: u128,
    pub intrinsic_payout_usdc: u64,
    pub current_sol_price_s6: i64,
    pub current_usdc_price_s6: i64,
    pub current_log_ratio_s6: i64,
    pub sigma_pricing_s6: i64,
    pub remaining_days: u32,
    pub engine_version: u16,
    pub now_ts: i64,
}

#[derive(Accounts)]
pub struct PreviewLendingValue<'info> {
    #[account(seeds = [seeds::PROTOCOL_CONFIG], seeds::program = halcyon_kernel::ID, bump)]
    pub protocol_config: Account<'info, ProtocolConfig>,
    #[account(
        seeds = [seeds::VAULT_SIGMA, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = vault_sigma.product_program_id == crate::ID @ KernelError::ProductProgramMismatch,
    )]
    pub vault_sigma: Account<'info, VaultSigma>,
    #[account(
        seeds = [seeds::REGIME_SIGNAL, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = regime_signal.product_program_id == crate::ID @ KernelError::ProductProgramMismatch,
    )]
    pub regime_signal: Account<'info, RegimeSignal>,
    #[account(
        constraint = policy_header.product_program_id == crate::ID @ KernelError::ProductProgramMismatch,
        constraint = policy_header.product_terms == product_terms.key() @ IlProtectionError::PolicyStateInvalid,
    )]
    pub policy_header: Account<'info, PolicyHeader>,
    #[account(
        constraint = product_terms.policy_header == policy_header.key() @ IlProtectionError::PolicyStateInvalid,
    )]
    pub product_terms: Account<'info, IlProtectionTerms>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_sol: UncheckedAccount<'info>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_usdc: UncheckedAccount<'info>,
    pub clock: Sysvar<'info, Clock>,
}

pub fn handler(ctx: Context<PreviewLendingValue>) -> Result<LendingValuePreview> {
    require!(
        ctx.accounts.policy_header.status == PolicyStatus::Active
            && ctx.accounts.product_terms.status == ProductStatus::Active,
        IlProtectionError::PolicyStateInvalid
    );

    let now = ctx.accounts.clock.unix_timestamp;
    require_sigma_fresh(
        &ctx.accounts.vault_sigma,
        now,
        ctx.accounts.protocol_config.sigma_staleness_cap_secs,
    )?;
    require_regime_fresh(
        &ctx.accounts.regime_signal,
        now,
        ctx.accounts.protocol_config.regime_staleness_cap_secs,
    )?;

    let pyth_sol = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_sol.to_account_info(),
        &halcyon_oracles::feed_ids::SOL_USD,
        &crate::ID,
        &ctx.accounts.clock,
        ctx.accounts.protocol_config.pyth_quote_staleness_cap_secs,
    )?;
    let pyth_usdc = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_usdc.to_account_info(),
        &halcyon_oracles::feed_ids::USDC_USD,
        &crate::ID,
        &ctx.accounts.clock,
        ctx.accounts.protocol_config.pyth_quote_staleness_cap_secs,
    )?;
    let sigma_pricing_s6 = compose_pricing_sigma(
        &ctx.accounts.vault_sigma,
        &ctx.accounts.regime_signal,
        protocol_sigma_floor_annualised_s6(&ctx.accounts.protocol_config),
    )?;

    let terms = &ctx.accounts.product_terms;
    let remaining_days = remaining_days_until_expiry(terms.expiry_ts, now)?;
    let nav = price_midlife_nav(&IlProtectionMidlifeInputs {
        weight_s12: terms.weight_s12,
        entry_sol_price_s6: terms.entry_sol_price_s6,
        entry_usdc_price_s6: terms.entry_usdc_price_s6,
        current_sol_price_s6: pyth_sol.price_s6,
        current_usdc_price_s6: pyth_usdc.price_s6,
        insured_notional_usdc: terms.insured_notional_usdc,
        deductible_s6: terms.deductible_s6,
        cap_s6: terms.cap_s6,
        sigma_annual_s6: sigma_pricing_s6,
        remaining_days,
    })
    .map_err(map_midlife_error)?;

    Ok(LendingValuePreview {
        nav_s6: nav.nav_s6,
        max_cover_s6: nav.max_cover_s6,
        lending_value_s6: nav.lending_value_s6,
        nav_payout_usdc: nav.nav_payout_usdc,
        lending_value_payout_usdc: nav.lending_value_payout_usdc,
        terminal_il_s6: nav.terminal_il_s6,
        terminal_il_s12: nav.terminal_il_s12,
        intrinsic_payout_usdc: nav.intrinsic_payout_usdc,
        current_sol_price_s6: pyth_sol.price_s6,
        current_usdc_price_s6: pyth_usdc.price_s6,
        current_log_ratio_s6: nav.current_log_ratio_s6,
        sigma_pricing_s6,
        remaining_days,
        engine_version: CURRENT_ENGINE_VERSION,
        now_ts: now,
    })
}

fn remaining_days_until_expiry(expiry_ts: i64, now: i64) -> Result<u32> {
    if expiry_ts <= now {
        return Ok(0);
    }
    let remaining_secs = expiry_ts.checked_sub(now).ok_or(HalcyonError::Overflow)?;
    let numerator = remaining_secs
        .checked_add(SECONDS_PER_DAY - 1)
        .ok_or(HalcyonError::Overflow)?;
    let days = numerator
        .checked_div(SECONDS_PER_DAY)
        .ok_or(HalcyonError::Overflow)?;
    u32::try_from(days).map_err(|_| error!(HalcyonError::Overflow))
}

fn map_midlife_error(_err: IlProtectionMidlifeError) -> Error {
    error!(IlProtectionError::MidlifePricingFailed)
}
