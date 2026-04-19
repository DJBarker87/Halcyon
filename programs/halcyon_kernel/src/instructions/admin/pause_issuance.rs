use anchor_lang::prelude::*;
use halcyon_common::{events::ConfigUpdated, seeds, HalcyonError};

use crate::state::*;

/// Shared context for toggling a single pause flag on `ProtocolConfig`.
#[derive(Accounts)]
pub struct SetPauseFlag<'info> {
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [seeds::PROTOCOL_CONFIG],
        bump,
        has_one = admin @ HalcyonError::AdminMismatch,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,
}

pub fn handler(ctx: Context<SetPauseFlag>, paused: bool) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let cfg = &mut ctx.accounts.protocol_config;
    cfg.issuance_paused_global = paused;
    cfg.last_update_ts = now;
    emit!(ConfigUpdated {
        admin: cfg.admin,
        field_tag: 1,
        updated_at: now,
    });
    Ok(())
}
