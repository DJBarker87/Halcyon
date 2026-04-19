use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};

use crate::state::*;

#[derive(Accounts)]
pub struct UpdateEwma<'info> {
    pub writer: Signer<'info>,

    #[account(seeds = [seeds::PROTOCOL_CONFIG], bump)]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(seeds = [seeds::KEEPER_REGISTRY], bump)]
    pub keeper_registry: Account<'info, KeeperRegistry>,

    #[account(
        mut,
        seeds = [seeds::VAULT_SIGMA, vault_sigma.product_program_id.as_ref()],
        bump,
    )]
    pub vault_sigma: Account<'info, VaultSigma>,
}

pub fn handler(ctx: Context<UpdateEwma>, ln_ratio_s12: i128) -> Result<()> {
    require_keys_eq!(
        ctx.accounts.writer.key(),
        ctx.accounts.keeper_registry.regime,
        HalcyonError::KeeperAuthorityMismatch
    );

    let clock = Clock::get()?;
    let now = clock.unix_timestamp;

    let rate_limit = ctx.accounts.protocol_config.ewma_rate_limit_secs;
    let last = ctx.accounts.vault_sigma.ewma_last_timestamp;

    require!(
        now.saturating_sub(last) >= rate_limit,
        HalcyonError::EwmaRateLimited
    );

    let sigma = &mut ctx.accounts.vault_sigma;

    // EWMA on variance with fixed λ = 0.94 (45-day span approximation).
    // var_new = λ·var_old + (1-λ)·r²
    // Operate in SCALE_12; 94% = 940_000_000_000 at s12.
    const LAMBDA_S12: i128 = 940_000_000_000;
    const ONE_MINUS_LAMBDA_S12: i128 = 60_000_000_000;
    const SCALE_12: i128 = 1_000_000_000_000;

    let r_squared_s12 = ln_ratio_s12
        .checked_mul(ln_ratio_s12)
        .ok_or(HalcyonError::Overflow)?
        .checked_div(SCALE_12)
        .ok_or(HalcyonError::Overflow)?;

    let weighted_old = sigma
        .ewma_var_daily_s12
        .checked_mul(LAMBDA_S12)
        .ok_or(HalcyonError::Overflow)?
        .checked_div(SCALE_12)
        .ok_or(HalcyonError::Overflow)?;
    let weighted_new = r_squared_s12
        .checked_mul(ONE_MINUS_LAMBDA_S12)
        .ok_or(HalcyonError::Overflow)?
        .checked_div(SCALE_12)
        .ok_or(HalcyonError::Overflow)?;

    sigma.ewma_var_daily_s12 = weighted_old
        .checked_add(weighted_new)
        .ok_or(HalcyonError::Overflow)?;
    sigma.ewma_last_ln_ratio_s12 = ln_ratio_s12;
    sigma.ewma_last_timestamp = now;
    sigma.last_update_slot = clock.slot;
    sigma.sample_count = sigma.sample_count.saturating_add(1);
    Ok(())
}
