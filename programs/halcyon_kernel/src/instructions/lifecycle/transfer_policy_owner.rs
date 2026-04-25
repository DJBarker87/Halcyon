use anchor_lang::prelude::*;
use halcyon_common::{events::PolicyOwnerTransferred, HalcyonError};

use crate::state::{PolicyHeader, PolicyStatus};

#[derive(Accounts)]
pub struct TransferPolicyOwner<'info> {
    pub current_owner: Signer<'info>,

    #[account(mut)]
    pub policy_header: Account<'info, PolicyHeader>,
}

pub fn handler(ctx: Context<TransferPolicyOwner>, new_owner: Pubkey) -> Result<()> {
    require_keys_eq!(
        ctx.accounts.policy_header.owner,
        ctx.accounts.current_owner.key(),
        HalcyonError::ProductAuthorityMismatch
    );
    require!(
        ctx.accounts.policy_header.status == PolicyStatus::Active,
        HalcyonError::PolicyNotActive
    );
    require_keys_neq!(
        new_owner,
        Pubkey::default(),
        HalcyonError::ProductAuthorityMismatch
    );

    let clock = Clock::get()?;
    let old_owner = ctx.accounts.policy_header.owner;
    ctx.accounts.policy_header.owner = new_owner;

    emit!(PolicyOwnerTransferred {
        policy_id: ctx.accounts.policy_header.key(),
        product_program_id: ctx.accounts.policy_header.product_program_id,
        old_owner,
        new_owner,
        transferred_at: clock.unix_timestamp,
    });

    Ok(())
}
