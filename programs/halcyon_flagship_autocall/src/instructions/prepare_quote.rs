use anchor_lang::prelude::*;
use halcyon_common::seeds;
use halcyon_kernel::state::{
    AutocallSchedule, ProductRegistryEntry, ProtocolConfig, Regression, VaultSigma,
};

use crate::pricing::{
    compose_pricing_sigma, require_autocall_schedule_fresh, require_correction_tables_match,
    require_protocol_unpaused, require_regression_fresh, require_sigma_fresh, solve_quote,
};
use crate::state::{FlagshipQuoteReceipt, CURRENT_ENGINE_VERSION};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct PrepareQuoteArgs {
    pub policy_id: Pubkey,
    pub notional_usdc: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct PreparedQuotePreview {
    pub premium: u64,
    pub max_liability: u64,
    pub fair_coupon_bps_s6: i64,
    pub offered_coupon_bps_s6: i64,
    pub sigma_pricing_s6: i64,
    pub quote_slot: u64,
    pub engine_version: u16,
    pub entry_spy_price_s6: i64,
    pub entry_qqq_price_s6: i64,
    pub entry_iwm_price_s6: i64,
    pub expiry_ts: i64,
}

#[derive(Accounts)]
#[instruction(args: PrepareQuoteArgs)]
pub struct PrepareQuote<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,
    #[account(init, payer = buyer, space = 8 + FlagshipQuoteReceipt::INIT_SPACE)]
    pub quote_receipt: Account<'info, FlagshipQuoteReceipt>,
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
    #[account(seeds = [seeds::REGRESSION], seeds::program = halcyon_kernel::ID, bump)]
    pub regression: Account<'info, Regression>,
    #[account(
        seeds = [seeds::AUTOCALL_SCHEDULE, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = autocall_schedule.product_program_id == crate::ID,
    )]
    pub autocall_schedule: Account<'info, AutocallSchedule>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_spy: UncheckedAccount<'info>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_qqq: UncheckedAccount<'info>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_iwm: UncheckedAccount<'info>,
    pub clock: Sysvar<'info, Clock>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<PrepareQuote>, args: PrepareQuoteArgs) -> Result<PreparedQuotePreview> {
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
    require_regression_fresh(
        &ctx.accounts.regression,
        now,
        ctx.accounts.protocol_config.regression_staleness_cap_secs,
    )?;
    require_autocall_schedule_fresh(&ctx.accounts.autocall_schedule, now)?;
    require_correction_tables_match(&ctx.accounts.protocol_config)?;

    let pyth_spy = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_spy.to_account_info(),
        &halcyon_oracles::feed_ids::SPY_USD,
        &crate::ID,
        &ctx.accounts.clock,
        ctx.accounts.protocol_config.pyth_quote_staleness_cap_secs,
    )?;
    let pyth_qqq = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_qqq.to_account_info(),
        &halcyon_oracles::feed_ids::QQQ_USD,
        &crate::ID,
        &ctx.accounts.clock,
        ctx.accounts.protocol_config.pyth_quote_staleness_cap_secs,
    )?;
    let pyth_iwm = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_iwm.to_account_info(),
        &halcyon_oracles::feed_ids::IWM_USD,
        &crate::ID,
        &ctx.accounts.clock,
        ctx.accounts.protocol_config.pyth_quote_staleness_cap_secs,
    )?;

    let sigma_pricing_s6 = compose_pricing_sigma(
        &ctx.accounts.vault_sigma,
        crate::pricing::protocol_sigma_floor_annualised_s6(&ctx.accounts.protocol_config),
        ctx.accounts.protocol_config.sigma_ceiling_annualised_s6,
    )?;
    let quote = solve_quote(
        sigma_pricing_s6,
        args.notional_usdc,
        &ctx.accounts.autocall_schedule,
    )?;

    ctx.accounts.quote_receipt.set_inner(FlagshipQuoteReceipt {
        version: FlagshipQuoteReceipt::CURRENT_VERSION,
        buyer: ctx.accounts.buyer.key(),
        policy_id: args.policy_id,
        notional_usdc: args.notional_usdc,
        premium: quote.premium,
        max_liability: quote.max_liability,
        fair_coupon_bps_s6: quote.fair_coupon_bps_s6,
        offered_coupon_bps_s6: quote.offered_coupon_bps_s6,
        sigma_pricing_s6: quote.sigma_pricing_s6,
        quote_slot: quote.quote_slot,
        entry_spy_price_s6: pyth_spy.price_s6,
        entry_qqq_price_s6: pyth_qqq.price_s6,
        entry_iwm_price_s6: pyth_iwm.price_s6,
        expiry_ts: quote.expiry_ts,
        created_at: now,
        beta_spy_s12: ctx.accounts.regression.beta_spy_s12,
        beta_qqq_s12: ctx.accounts.regression.beta_qqq_s12,
        alpha_s12: ctx.accounts.regression.alpha_s12,
        regression_r_squared_s6: ctx.accounts.regression.r_squared_s6,
        regression_residual_vol_s6: ctx.accounts.regression.residual_vol_s6,
    });

    Ok(PreparedQuotePreview {
        premium: quote.premium,
        max_liability: quote.max_liability,
        fair_coupon_bps_s6: quote.fair_coupon_bps_s6,
        offered_coupon_bps_s6: quote.offered_coupon_bps_s6,
        sigma_pricing_s6: quote.sigma_pricing_s6,
        quote_slot: quote.quote_slot,
        engine_version: CURRENT_ENGINE_VERSION,
        entry_spy_price_s6: pyth_spy.price_s6,
        entry_qqq_price_s6: pyth_qqq.price_s6,
        entry_iwm_price_s6: pyth_iwm.price_s6,
        expiry_ts: quote.expiry_ts,
    })
}
