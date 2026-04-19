use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};
use solmath_core::{ln_fixed_i, SCALE};

use crate::state::*;

#[derive(Accounts)]
pub struct UpdateEwma<'info> {
    #[account(seeds = [seeds::PROTOCOL_CONFIG], bump)]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(
        mut,
        seeds = [seeds::VAULT_SIGMA, vault_sigma.product_program_id.as_ref()],
        bump,
    )]
    pub vault_sigma: Account<'info, VaultSigma>,

    /// CHECK: validated by `halcyon_oracles`.
    pub oracle_price: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<UpdateEwma>) -> Result<()> {
    let clock = Clock::get()?;
    let snapshot = halcyon_oracles::read_pyth_price(
        &ctx.accounts.oracle_price.to_account_info(),
        &ctx.accounts.vault_sigma.oracle_feed_id,
        &crate::ID,
        &clock,
        ctx.accounts.protocol_config.pyth_quote_staleness_cap_secs,
    )?;

    let sigma = &mut ctx.accounts.vault_sigma;
    require!(
        snapshot.price_s6 > 0,
        crate::KernelError::InvalidOraclePrice
    );

    if sigma.sample_count == 0 {
        sigma.ewma_var_daily_s12 = 0;
        sigma.ewma_last_ln_ratio_s12 = 0;
        sigma.ewma_last_timestamp = snapshot.publish_ts;
        sigma.last_price_s6 = snapshot.price_s6;
        sigma.last_publish_ts = snapshot.publish_ts;
        sigma.last_publish_slot = snapshot.publish_slot;
        sigma.last_update_slot = snapshot.publish_slot;
        sigma.sample_count = 1;
        return Ok(());
    }

    require!(
        sigma.last_price_s6 > 0,
        crate::KernelError::InvalidOraclePrice
    );
    let dt = snapshot
        .publish_ts
        .checked_sub(sigma.last_publish_ts)
        .ok_or(HalcyonError::Overflow)?;
    require!(
        dt >= ctx.accounts.protocol_config.ewma_rate_limit_secs,
        HalcyonError::EwmaRateLimited
    );
    require!(
        snapshot.publish_ts > sigma.last_publish_ts
            || (snapshot.publish_ts == sigma.last_publish_ts
                && snapshot.publish_slot > sigma.last_publish_slot),
        HalcyonError::OracleTimestampNotMonotonic
    );

    // EWMA on variance with fixed λ = 0.94 (45-day span approximation).
    // var_new = λ·var_old + (1-λ)·r²
    // Operate in SCALE_12; 94% = 940_000_000_000 at s12.
    const LAMBDA_S12: i128 = 940_000_000_000;
    const ONE_MINUS_LAMBDA_S12: i128 = 60_000_000_000;
    let ratio_s12 = u128::try_from(snapshot.price_s6)
        .map_err(|_| error!(crate::KernelError::InvalidOraclePrice))?
        .checked_mul(SCALE)
        .ok_or(HalcyonError::Overflow)?
        .checked_div(
            u128::try_from(sigma.last_price_s6)
                .map_err(|_| error!(crate::KernelError::InvalidOraclePrice))?,
        )
        .ok_or(HalcyonError::Overflow)?;
    let ln_ratio_s12 =
        ln_fixed_i(ratio_s12).map_err(|_| error!(crate::KernelError::InvalidOraclePrice))?;

    let r_squared_s12 = ln_ratio_s12
        .checked_mul(ln_ratio_s12)
        .ok_or(HalcyonError::Overflow)?
        .checked_div(SCALE as i128)
        .ok_or(HalcyonError::Overflow)?;

    let weighted_old = sigma
        .ewma_var_daily_s12
        .checked_mul(LAMBDA_S12)
        .ok_or(HalcyonError::Overflow)?
        .checked_div(SCALE as i128)
        .ok_or(HalcyonError::Overflow)?;
    let weighted_new = r_squared_s12
        .checked_mul(ONE_MINUS_LAMBDA_S12)
        .ok_or(HalcyonError::Overflow)?
        .checked_div(SCALE as i128)
        .ok_or(HalcyonError::Overflow)?;

    sigma.ewma_var_daily_s12 = weighted_old
        .checked_add(weighted_new)
        .ok_or(HalcyonError::Overflow)?;
    sigma.ewma_last_ln_ratio_s12 = ln_ratio_s12;
    sigma.ewma_last_timestamp = snapshot.publish_ts;
    sigma.last_price_s6 = snapshot.price_s6;
    sigma.last_publish_ts = snapshot.publish_ts;
    sigma.last_publish_slot = snapshot.publish_slot;
    sigma.last_update_slot = snapshot.publish_slot;
    sigma.sample_count = sigma.sample_count.saturating_add(1);
    Ok(())
}
