use anchor_lang::prelude::*;
use halcyon_common::{events::KeeperRotated, seeds, HalcyonError};

use crate::{state::*, KernelError};

#[derive(Accounts)]
pub struct RotateKeeper<'info> {
    pub admin: Signer<'info>,

    #[account(
        seeds = [seeds::PROTOCOL_CONFIG],
        bump,
        has_one = admin @ HalcyonError::AdminMismatch,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(mut, seeds = [seeds::KEEPER_REGISTRY], bump)]
    pub keeper_registry: Account<'info, KeeperRegistry>,
}

pub fn handler(ctx: Context<RotateKeeper>, role: u8, new_authority: Pubkey) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let registry = &mut ctx.accounts.keeper_registry;
    let old = match role {
        0 => {
            let old = registry.observation;
            registry.observation = new_authority;
            old
        }
        1 => {
            let old = registry.regression;
            registry.regression = new_authority;
            old
        }
        2 => {
            let old = registry.delta;
            registry.delta = new_authority;
            old
        }
        3 => {
            let old = registry.hedge;
            registry.hedge = new_authority;
            old
        }
        4 => {
            let old = registry.regime;
            registry.regime = new_authority;
            old
        }
        _ => return err!(KernelError::InvalidKeeperRole),
    };
    registry.last_rotation_ts = now;
    emit!(KeeperRotated {
        role,
        old_authority: old,
        new_authority,
        rotated_at: now,
    });
    Ok(())
}
