use anchor_lang::prelude::*;
use halcyon_common::{product_ids, seeds, HalcyonError};
use solmath_core::{ln_fixed_i, SCALE};

use crate::state::*;

/// L-1 — `update_ewma` is intentionally permissionless. The authenticity
/// gate is the full Pyth read (`read_pyth_price`: owner check, feed-id
/// match, full-verification level, staleness cap) combined with the
/// per-product `ewma_rate_limit_secs` throttle and the strict monotonicity
/// on `publish_ts` / `publish_slot`. Any caller paying a rent-free tx may
/// advance the EWMA; no signer is required because no signer could bypass
/// these checks anyway.
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

fn rate_limit_secs(cfg: &ProtocolConfig, product_program_id: &Pubkey) -> Result<i64> {
    let rate_limit = if *product_program_id == product_ids::IL_PROTECTION {
        cfg.il_ewma_rate_limit_secs
    } else if *product_program_id == product_ids::SOL_AUTOCALL {
        cfg.sol_autocall_ewma_rate_limit_secs
    } else {
        u64::try_from(cfg.ewma_rate_limit_secs)
            .map_err(|_| error!(crate::KernelError::BadConfig))?
    };
    i64::try_from(rate_limit).map_err(|_| error!(crate::KernelError::BadConfig))
}

pub fn handler(ctx: Context<UpdateEwma>) -> Result<()> {
    let clock = Clock::get()?;
    let product_program_id = ctx.accounts.vault_sigma.product_program_id;
    let sigma_floor_s6 = ctx
        .accounts
        .protocol_config
        .sigma_floor_for_product_s6(&product_program_id);
    require!(sigma_floor_s6 > 0, crate::KernelError::BadConfig);
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
    let rate_limit_secs = rate_limit_secs(&ctx.accounts.protocol_config, &product_program_id)?;
    require!(dt >= rate_limit_secs, HalcyonError::EwmaRateLimited);
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

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::prelude::Pubkey;

    fn sample_protocol_config() -> ProtocolConfig {
        ProtocolConfig {
            version: ProtocolConfig::CURRENT_VERSION,
            admin: Pubkey::default(),
            issuance_paused_global: false,
            settlement_paused_global: false,
            utilization_cap_bps: 9_000,
            senior_share_bps: 9_000,
            junior_share_bps: 300,
            treasury_share_bps: 700,
            senior_cooldown_secs: 0,
            ewma_rate_limit_secs: 3_600,
            il_ewma_rate_limit_secs: 3_600,
            sol_autocall_ewma_rate_limit_secs: 86_400,
            sigma_staleness_cap_secs: 3_600,
            regime_staleness_cap_secs: 86_400,
            regression_staleness_cap_secs: 86_400,
            pyth_quote_staleness_cap_secs: 30,
            pyth_settle_staleness_cap_secs: 60,
            quote_ttl_secs: 300,
            sigma_floor_annualised_s6: 400_000,
            il_sigma_floor_annualised_s6: 400_000,
            sol_autocall_sigma_floor_annualised_s6: 400_000,
            flagship_sigma_floor_annualised_s6: 400_000,
            sigma_ceiling_annualised_s6: 800_000,
            sol_autocall_quote_share_bps: 7_500,
            sol_autocall_issuer_margin_bps: 50,
            k12_correction_sha256: [0u8; 32],
            daily_ki_correction_sha256: [0u8; 32],
            pod_deim_table_sha256: [0u8; 32],
            treasury_destination: Pubkey::default(),
            hedge_max_slippage_bps_cap: 100,
            hedge_defund_destination: Pubkey::default(),
            last_update_ts: 0,
        }
    }

    #[test]
    fn picks_il_specific_rate_limit() {
        let cfg = sample_protocol_config();
        let limit = rate_limit_secs(&cfg, &product_ids::IL_PROTECTION).unwrap();
        assert_eq!(limit, 3_600);
    }

    #[test]
    fn picks_sol_specific_rate_limit() {
        let cfg = sample_protocol_config();
        let limit = rate_limit_secs(&cfg, &product_ids::SOL_AUTOCALL).unwrap();
        assert_eq!(limit, 86_400);
    }

    #[test]
    fn falls_back_for_other_products() {
        let cfg = sample_protocol_config();
        let limit = rate_limit_secs(&cfg, &product_ids::FLAGSHIP_AUTOCALL).unwrap();
        assert_eq!(limit, 3_600);
    }
}
