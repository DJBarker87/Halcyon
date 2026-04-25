use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};
use halcyon_kernel::state::{PolicyHeader, PolicyStatus};

use crate::errors::FlagshipAutocallError;
use crate::state::{
    FlagshipAutocallTerms, ProductStatus, RetailRedemptionRequest, RETAIL_REDEMPTION_EXPIRY_SECS,
    RETAIL_REDEMPTION_NOTICE_SECS,
};

#[event]
pub struct FlagshipRetailRedemptionRequested {
    pub policy_id: Pubkey,
    pub owner: Pubkey,
    pub requested_at: i64,
    pub earliest_execute_ts: i64,
    pub expires_at: i64,
}

#[derive(Accounts)]
pub struct RequestRetailRedemption<'info> {
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
        init,
        payer = policy_owner,
        space = 8 + RetailRedemptionRequest::INIT_SPACE,
        seeds = [seeds::RETAIL_REDEMPTION, policy_header.key().as_ref()],
        bump,
    )]
    pub redemption_request: Box<Account<'info, RetailRedemptionRequest>>,

    pub clock: Sysvar<'info, Clock>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<RequestRetailRedemption>) -> Result<()> {
    require!(
        ctx.accounts.policy_header.status == PolicyStatus::Active
            && ctx.accounts.product_terms.status == ProductStatus::Active,
        FlagshipAutocallError::PolicyStateInvalid
    );

    let now = ctx.accounts.clock.unix_timestamp;
    let earliest_execute_ts = now
        .checked_add(RETAIL_REDEMPTION_NOTICE_SECS)
        .ok_or(HalcyonError::Overflow)?;
    let expires_at = earliest_execute_ts
        .checked_add(RETAIL_REDEMPTION_EXPIRY_SECS)
        .ok_or(HalcyonError::Overflow)?;

    ctx.accounts
        .redemption_request
        .set_inner(RetailRedemptionRequest {
            version: RetailRedemptionRequest::CURRENT_VERSION,
            policy_header: ctx.accounts.policy_header.key(),
            requester: ctx.accounts.policy_owner.key(),
            requested_at: now,
            earliest_execute_ts,
            expires_at,
        });

    emit!(FlagshipRetailRedemptionRequested {
        policy_id: ctx.accounts.policy_header.key(),
        owner: ctx.accounts.policy_owner.key(),
        requested_at: now,
        earliest_execute_ts,
        expires_at,
    });

    Ok(())
}
