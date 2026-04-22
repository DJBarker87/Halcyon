use anchor_lang::prelude::*;
use halcyon_common::seeds;
use halcyon_kernel::state::{ProductRegistryEntry, ProtocolConfig, RegimeSignal, VaultSigma};

use crate::pricing::{
    compose_pricing_sigma, regime_kind_tag, require_protocol_unpaused, require_regime_fresh,
    require_sigma_fresh, solve_quote,
};
use crate::state::CURRENT_ENGINE_VERSION;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct QuotePreview {
    pub premium: u64,
    pub max_liability: u64,
    pub fair_premium_fraction_s6: i64,
    pub loaded_premium_fraction_s6: i64,
    pub sigma_pricing_s6: i64,
    pub fvol_s6: i64,
    pub regime: u8,
    pub sigma_multiplier_s6: i64,
    pub quote_slot: u64,
    pub engine_version: u16,
    pub entry_sol_price_s6: i64,
    pub entry_usdc_price_s6: i64,
    pub expiry_ts: i64,
}

#[derive(Accounts)]
pub struct PreviewQuote<'info> {
    #[account(seeds = [seeds::PROTOCOL_CONFIG], seeds::program = halcyon_kernel::ID, bump)]
    pub protocol_config: Account<'info, ProtocolConfig>,
    #[account(
        seeds = [seeds::PRODUCT_REGISTRY, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = product_registry_entry.product_program_id == crate::ID,
        constraint = product_registry_entry.active @ halcyon_common::HalcyonError::ProductNotRegistered,
    )]
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,
    #[account(
        seeds = [seeds::VAULT_SIGMA, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = vault_sigma.product_program_id == crate::ID,
    )]
    pub vault_sigma: Account<'info, VaultSigma>,
    #[account(
        seeds = [seeds::REGIME_SIGNAL, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = regime_signal.product_program_id == crate::ID,
    )]
    pub regime_signal: Account<'info, RegimeSignal>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_sol: UncheckedAccount<'info>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_usdc: UncheckedAccount<'info>,
    pub clock: Sysvar<'info, Clock>,
}

pub fn handler(ctx: Context<PreviewQuote>, insured_notional_usdc: u64) -> Result<QuotePreview> {
    let now = ctx.accounts.clock.unix_timestamp;

    require_protocol_unpaused(&ctx.accounts.protocol_config)?;
    require!(
        !ctx.accounts.product_registry_entry.paused,
        halcyon_common::HalcyonError::IssuancePausedPerProduct
    );
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
        crate::pricing::protocol_sigma_floor_annualised_s6(&ctx.accounts.protocol_config),
    )?;
    let quote = solve_quote(sigma_pricing_s6, insured_notional_usdc, now)?;

    Ok(QuotePreview {
        premium: quote.premium,
        max_liability: quote.max_liability,
        fair_premium_fraction_s6: quote.fair_premium_fraction_s6,
        loaded_premium_fraction_s6: quote.loaded_premium_fraction_s6,
        sigma_pricing_s6: quote.sigma_pricing_s6,
        fvol_s6: ctx.accounts.regime_signal.fvol_s6,
        regime: regime_kind_tag(&ctx.accounts.regime_signal),
        sigma_multiplier_s6: ctx.accounts.regime_signal.sigma_multiplier_s6,
        quote_slot: quote.quote_slot,
        engine_version: CURRENT_ENGINE_VERSION,
        entry_sol_price_s6: pyth_sol.price_s6,
        entry_usdc_price_s6: pyth_usdc.price_s6,
        expiry_ts: quote.expiry_ts,
    })
}
