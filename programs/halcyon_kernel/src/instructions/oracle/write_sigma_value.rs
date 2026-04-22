use anchor_lang::prelude::*;
use halcyon_common::{product_ids, seeds, HalcyonError};

use crate::state::{KeeperRegistry, ProductRegistryEntry, ProtocolConfig, VaultSigma};

const TRADING_DAYS_PER_YEAR: i128 = 252;
const MAX_SIGMA_CLOCK_SKEW_SECS: i64 = 5;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct WriteSigmaValueArgs {
    pub sigma_annualised_s6: i64,
    pub publish_ts: i64,
    pub publish_slot: u64,
}

#[derive(Accounts)]
pub struct WriteSigmaValue<'info> {
    pub keeper: Signer<'info>,

    #[account(seeds = [seeds::PROTOCOL_CONFIG], bump)]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(seeds = [seeds::KEEPER_REGISTRY], bump)]
    pub keeper_registry: Account<'info, KeeperRegistry>,

    #[account(
        seeds = [seeds::PRODUCT_REGISTRY, product_ids::FLAGSHIP_AUTOCALL.as_ref()],
        bump,
        constraint = product_registry_entry.product_program_id == product_ids::FLAGSHIP_AUTOCALL
            @ crate::KernelError::ProductProgramMismatch,
        constraint = product_registry_entry.active @ HalcyonError::ProductNotRegistered,
    )]
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,

    #[account(
        mut,
        seeds = [seeds::VAULT_SIGMA, product_ids::FLAGSHIP_AUTOCALL.as_ref()],
        bump,
        constraint = vault_sigma.product_program_id == product_ids::FLAGSHIP_AUTOCALL
            @ crate::KernelError::ProductProgramMismatch,
    )]
    pub vault_sigma: Account<'info, VaultSigma>,
}

pub fn handler(ctx: Context<WriteSigmaValue>, args: WriteSigmaValueArgs) -> Result<()> {
    require_keys_eq!(
        ctx.accounts.keeper.key(),
        ctx.accounts.keeper_registry.observation,
        HalcyonError::KeeperAuthorityMismatch
    );

    let cfg = &ctx.accounts.protocol_config;
    require!(cfg.sigma_bounds_valid(), crate::KernelError::BadConfig);
    let sigma_floor_s6 = cfg.sigma_floor_for_product_s6(&ctx.accounts.vault_sigma.product_program_id);
    require!(
        args.sigma_annualised_s6 >= sigma_floor_s6
            && args.sigma_annualised_s6 <= cfg.sigma_ceiling_annualised_s6,
        crate::KernelError::BadConfig
    );

    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    require!(
        args.publish_ts <= now.saturating_add(MAX_SIGMA_CLOCK_SKEW_SECS),
        HalcyonError::SigmaStale
    );
    require!(
        now.saturating_sub(args.publish_ts) <= cfg.sigma_staleness_cap_secs,
        HalcyonError::SigmaStale
    );

    let sigma = &mut ctx.accounts.vault_sigma;
    require!(
        args.publish_ts > sigma.last_publish_ts,
        HalcyonError::OracleTimestampNotMonotonic
    );

    let annual_variance_s12 = i128::from(args.sigma_annualised_s6)
        .checked_mul(i128::from(args.sigma_annualised_s6))
        .ok_or(HalcyonError::Overflow)?;
    let daily_variance_s12 = annual_variance_s12
        .checked_div(TRADING_DAYS_PER_YEAR)
        .ok_or(HalcyonError::Overflow)?;

    sigma.ewma_var_daily_s12 = daily_variance_s12;
    sigma.ewma_last_ln_ratio_s12 = 0;
    sigma.ewma_last_timestamp = args.publish_ts;
    sigma.last_price_s6 = 0;
    sigma.last_publish_ts = args.publish_ts;
    sigma.last_publish_slot = args.publish_slot;
    sigma.last_update_slot = args.publish_slot;
    sigma.sample_count = sigma.sample_count.saturating_add(1).max(1);
    Ok(())
}
