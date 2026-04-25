use anchor_lang::prelude::*;
use halcyon_kernel::state::{PolicyHeader, PolicyStatus};
use halcyon_kernel::KernelError;

use crate::errors::FlagshipAutocallError;
use crate::instructions::prepare_midlife_nav::MidlifeNavCheckpointPreview;
use crate::midlife_pricing;
use crate::state::{FlagshipAutocallTerms, ProductStatus};

#[derive(Accounts)]
pub struct AdvanceMidlifeNav<'info> {
    pub requester: Signer<'info>,

    /// CHECK: owned by this program and manually validated as a midlife
    /// checkpoint byte account.
    #[account(mut)]
    pub midlife_checkpoint: UncheckedAccount<'info>,

    #[account(
        constraint = policy_header.product_program_id == crate::ID @ KernelError::ProductProgramMismatch,
        constraint = policy_header.product_terms == product_terms.key() @ FlagshipAutocallError::PolicyStateInvalid,
    )]
    pub policy_header: Box<Account<'info, PolicyHeader>>,

    #[account(
        constraint = product_terms.policy_header == policy_header.key() @ FlagshipAutocallError::PolicyStateInvalid,
    )]
    pub product_terms: Box<Account<'info, FlagshipAutocallTerms>>,

    pub clock: Sysvar<'info, Clock>,
}

pub fn handler(
    ctx: Context<AdvanceMidlifeNav>,
    stop_coupon_index: u8,
) -> Result<MidlifeNavCheckpointPreview> {
    require!(
        ctx.accounts.policy_header.status == PolicyStatus::Active
            && ctx.accounts.product_terms.status == ProductStatus::Active,
        FlagshipAutocallError::PolicyStateInvalid
    );
    midlife_pricing::validate_checkpoint_account(
        &ctx.accounts.midlife_checkpoint.to_account_info(),
        ctx.accounts.requester.key(),
        ctx.accounts.policy_header.key(),
        ctx.accounts.product_terms.key(),
        ctx.accounts.clock.slot,
    )?;
    midlife_pricing::require_checkpoint_matches_policy_state(
        &ctx.accounts.midlife_checkpoint.to_account_info(),
        &ctx.accounts.policy_header,
        &ctx.accounts.product_terms,
    )?;

    let view = midlife_pricing::advance_checkpoint(
        &ctx.accounts.midlife_checkpoint.to_account_info(),
        stop_coupon_index,
    )?;

    Ok(MidlifeNavCheckpointPreview {
        next_coupon_index: view.next_coupon_index,
        final_coupon_index: view.final_coupon_index,
        prepared_slot: view.prepared_slot,
        expires_at_slot: view.expires_at_slot,
        sigma_pricing_s6: view.inputs.sigma_common_s6,
        now_trading_day: view.inputs.now_trading_day,
    })
}
