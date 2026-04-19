use anchor_lang::prelude::*;
use halcyon_common::{events::ConfigUpdated, seeds, HalcyonError};

use crate::{state::*, KernelError};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct PremiumSplitsBps {
    pub senior_bps: u16,
    pub junior_bps: u16,
    pub treasury_bps: u16,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct SetProtocolConfigArgs {
    pub utilization_cap_bps: Option<u64>,
    pub sigma_staleness_cap_secs: Option<i64>,
    pub regime_staleness_cap_secs: Option<i64>,
    pub regression_staleness_cap_secs: Option<i64>,
    pub pyth_quote_staleness_cap_secs: Option<i64>,
    pub pyth_settle_staleness_cap_secs: Option<i64>,
    pub ewma_rate_limit_secs: Option<i64>,
    pub senior_cooldown_secs: Option<i64>,
    pub sigma_floor_annualised_s6: Option<i64>,
    pub k12_correction_sha256: Option<[u8; 32]>,
    pub daily_ki_correction_sha256: Option<[u8; 32]>,
    pub premium_splits_bps: Option<PremiumSplitsBps>,
    pub treasury_destination: Option<Pubkey>,
}

#[derive(Accounts)]
pub struct SetProtocolConfig<'info> {
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [seeds::PROTOCOL_CONFIG],
        bump,
        has_one = admin @ HalcyonError::AdminMismatch,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,
}

pub fn handler(ctx: Context<SetProtocolConfig>, args: SetProtocolConfigArgs) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let cfg = &mut ctx.accounts.protocol_config;

    if let Some(x) = args.utilization_cap_bps {
        cfg.utilization_cap_bps = x;
    }
    if let Some(x) = args.sigma_staleness_cap_secs {
        cfg.sigma_staleness_cap_secs = x;
    }
    if let Some(x) = args.regime_staleness_cap_secs {
        cfg.regime_staleness_cap_secs = x;
    }
    if let Some(x) = args.regression_staleness_cap_secs {
        cfg.regression_staleness_cap_secs = x;
    }
    if let Some(x) = args.pyth_quote_staleness_cap_secs {
        cfg.pyth_quote_staleness_cap_secs = x;
    }
    if let Some(x) = args.pyth_settle_staleness_cap_secs {
        cfg.pyth_settle_staleness_cap_secs = x;
    }
    if let Some(x) = args.ewma_rate_limit_secs {
        cfg.ewma_rate_limit_secs = x;
    }
    if let Some(x) = args.senior_cooldown_secs {
        cfg.senior_cooldown_secs = x;
    }
    if let Some(x) = args.sigma_floor_annualised_s6 {
        cfg.sigma_floor_annualised_s6 = x;
    }
    if let Some(h) = args.k12_correction_sha256 {
        cfg.k12_correction_sha256 = h;
    }
    if let Some(h) = args.daily_ki_correction_sha256 {
        cfg.daily_ki_correction_sha256 = h;
    }
    if let Some(splits) = args.premium_splits_bps {
        cfg.senior_share_bps = splits.senior_bps;
        cfg.junior_share_bps = splits.junior_bps;
        cfg.treasury_share_bps = splits.treasury_bps;
        require!(
            cfg.premium_splits_sum_to_ten_thousand(),
            KernelError::BadConfig
        );
    }
    if let Some(dst) = args.treasury_destination {
        require_keys_neq!(dst, Pubkey::default(), HalcyonError::DestinationNotAllowed);
        cfg.treasury_destination = dst;
    }

    cfg.last_update_ts = now;
    emit!(ConfigUpdated {
        admin: cfg.admin,
        field_tag: 0,
        updated_at: now,
    });
    Ok(())
}
