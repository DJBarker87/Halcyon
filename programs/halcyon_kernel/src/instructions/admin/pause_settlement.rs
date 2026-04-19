use anchor_lang::prelude::*;
use halcyon_common::events::ConfigUpdated;

use crate::instructions::admin::pause_issuance::SetPauseFlag;

pub fn handler(ctx: Context<SetPauseFlag>, paused: bool) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let cfg = &mut ctx.accounts.protocol_config;
    cfg.settlement_paused_global = paused;
    cfg.last_update_ts = now;
    emit!(ConfigUpdated {
        admin: cfg.admin,
        field_tag: 2,
        updated_at: now,
    });
    Ok(())
}
