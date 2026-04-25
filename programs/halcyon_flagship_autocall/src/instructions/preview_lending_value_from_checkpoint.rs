use anchor_lang::prelude::*;
use halcyon_kernel::state::{PolicyHeader, PolicyStatus};
use halcyon_kernel::KernelError;

use crate::buyback_math::{
    lending_value_payout_usdc, lending_value_s6 as compute_lending_value_s6,
};
use crate::errors::FlagshipAutocallError;
use crate::instructions::preview_lending_value::LendingValuePreview;
use crate::midlife_pricing;
use crate::state::{FlagshipAutocallTerms, ProductStatus};

#[derive(Accounts)]
pub struct PreviewLendingValueFromCheckpoint<'info> {
    #[account(mut)]
    pub requester: Signer<'info>,

    /// CHECK: owned by this program and manually validated/closed as a
    /// midlife checkpoint byte account.
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

pub fn handler(ctx: Context<PreviewLendingValueFromCheckpoint>) -> Result<LendingValuePreview> {
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

    let valuation = midlife_pricing::finish_nav_from_checkpoint(
        &ctx.accounts.midlife_checkpoint.to_account_info(),
    )?;
    let lending_value_s6 =
        compute_lending_value_s6(valuation.nav.nav_s6, valuation.nav.ki_level_usd_s6);
    let lending_value_payout_usdc =
        lending_value_payout_usdc(ctx.accounts.policy_header.notional, lending_value_s6)?;
    midlife_pricing::close_checkpoint_account(
        &ctx.accounts.midlife_checkpoint.to_account_info(),
        &ctx.accounts.requester.to_account_info(),
    )?;

    Ok(LendingValuePreview {
        nav_s6: valuation.nav.nav_s6,
        ki_level_usd_s6: valuation.nav.ki_level_usd_s6,
        lending_value_s6,
        lending_value_payout_usdc,
        remaining_coupon_pv_s6: valuation.nav.remaining_coupon_pv_s6,
        par_recovery_probability_s6: valuation.nav.par_recovery_probability_s6,
        sigma_pricing_s6: valuation.sigma_pricing_s6,
        now_trading_day: valuation.now_trading_day,
    })
}
