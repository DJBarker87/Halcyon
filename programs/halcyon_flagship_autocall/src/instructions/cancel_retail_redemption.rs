use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};
use halcyon_kernel::state::{PolicyHeader, PolicyStatus};

use crate::errors::FlagshipAutocallError;
use crate::state::{FlagshipAutocallTerms, ProductStatus, RetailRedemptionRequest};

#[event]
pub struct FlagshipRetailRedemptionCancelled {
    pub policy_id: Pubkey,
    pub owner: Pubkey,
    pub cancelled_at: i64,
}

#[derive(Accounts)]
pub struct CancelRetailRedemption<'info> {
    #[account(mut)]
    pub policy_owner: Signer<'info>,

    #[account(
        constraint = policy_header.product_program_id == crate::ID
            @ halcyon_kernel::KernelError::ProductProgramMismatch,
        constraint = policy_header.owner == policy_owner.key()
            @ HalcyonError::ProductAuthorityMismatch,
        constraint = policy_header.product_terms == product_terms.key()
            @ FlagshipAutocallError::PolicyStateInvalid,
    )]
    pub policy_header: Box<Account<'info, PolicyHeader>>,

    #[account(
        constraint = product_terms.policy_header == policy_header.key()
            @ FlagshipAutocallError::PolicyStateInvalid,
    )]
    pub product_terms: Box<Account<'info, FlagshipAutocallTerms>>,

    #[account(
        mut,
        close = policy_owner,
        seeds = [seeds::RETAIL_REDEMPTION, policy_header.key().as_ref()],
        bump,
        constraint = redemption_request.policy_header == policy_header.key()
            @ FlagshipAutocallError::PolicyStateInvalid,
        constraint = redemption_request.requester == policy_owner.key()
            @ HalcyonError::ProductAuthorityMismatch,
    )]
    pub redemption_request: Box<Account<'info, RetailRedemptionRequest>>,

    pub clock: Sysvar<'info, Clock>,
}

pub fn handler(ctx: Context<CancelRetailRedemption>) -> Result<()> {
    require!(
        ctx.accounts.policy_header.status == PolicyStatus::Active
            && ctx.accounts.product_terms.status == ProductStatus::Active,
        FlagshipAutocallError::PolicyStateInvalid
    );

    emit!(FlagshipRetailRedemptionCancelled {
        policy_id: ctx.accounts.policy_header.key(),
        owner: ctx.accounts.policy_owner.key(),
        cancelled_at: ctx.accounts.clock.unix_timestamp,
    });

    Ok(())
}
