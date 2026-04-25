use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};
use halcyon_kernel::state::{PolicyHeader, PolicyStatus, ProtocolConfig, Regression, VaultSigma};
use halcyon_kernel::KernelError;

use crate::errors::FlagshipAutocallError;
use crate::midlife_pricing;
use crate::state::{FlagshipAutocallTerms, ProductStatus, MIDLIFE_NAV_CHECKPOINT_EXPIRY_SLOTS};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct MidlifeNavCheckpointPreview {
    pub next_coupon_index: u8,
    pub final_coupon_index: u8,
    pub prepared_slot: u64,
    pub expires_at_slot: u64,
    pub sigma_pricing_s6: i64,
    pub now_trading_day: u16,
}

#[derive(Accounts)]
pub struct PrepareMidlifeNav<'info> {
    pub requester: Signer<'info>,

    /// CHECK: owned by this program and manually initialized as a fixed-size
    /// midlife checkpoint byte account.
    #[account(mut)]
    pub midlife_checkpoint: UncheckedAccount<'info>,

    #[account(seeds = [seeds::PROTOCOL_CONFIG], seeds::program = halcyon_kernel::ID, bump)]
    pub protocol_config: Box<Account<'info, ProtocolConfig>>,

    #[account(
        seeds = [seeds::VAULT_SIGMA, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = vault_sigma.product_program_id == crate::ID @ KernelError::ProductProgramMismatch,
    )]
    pub vault_sigma: Box<Account<'info, VaultSigma>>,

    #[account(seeds = [seeds::REGRESSION], seeds::program = halcyon_kernel::ID, bump)]
    pub regression: Box<Account<'info, Regression>>,

    #[account(
        constraint = policy_header.product_program_id == crate::ID @ KernelError::ProductProgramMismatch,
        constraint = policy_header.product_terms == product_terms.key() @ FlagshipAutocallError::PolicyStateInvalid,
    )]
    pub policy_header: Box<Account<'info, PolicyHeader>>,

    #[account(
        constraint = product_terms.policy_header == policy_header.key() @ FlagshipAutocallError::PolicyStateInvalid,
    )]
    pub product_terms: Box<Account<'info, FlagshipAutocallTerms>>,

    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_spy: UncheckedAccount<'info>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_qqq: UncheckedAccount<'info>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_iwm: UncheckedAccount<'info>,

    pub clock: Sysvar<'info, Clock>,
}

pub fn handler(
    ctx: Context<PrepareMidlifeNav>,
    stop_coupon_index: u8,
) -> Result<MidlifeNavCheckpointPreview> {
    require!(
        ctx.accounts.policy_header.status == PolicyStatus::Active
            && ctx.accounts.product_terms.status == ProductStatus::Active,
        FlagshipAutocallError::PolicyStateInvalid
    );

    let prepared = midlife_pricing::prepare_nav_from_accounts(
        &ctx.accounts.protocol_config,
        &ctx.accounts.vault_sigma,
        &ctx.accounts.regression,
        &ctx.accounts.policy_header,
        &ctx.accounts.product_terms,
        &ctx.accounts.pyth_spy.to_account_info(),
        &ctx.accounts.pyth_qqq.to_account_info(),
        &ctx.accounts.pyth_iwm.to_account_info(),
        &ctx.accounts.clock,
        stop_coupon_index,
    )?;
    let prepared_slot = ctx.accounts.clock.slot;
    let expires_at_slot = prepared_slot
        .checked_add(MIDLIFE_NAV_CHECKPOINT_EXPIRY_SLOTS)
        .ok_or(HalcyonError::Overflow)?;
    let view = midlife_pricing::write_checkpoint_account_from_inputs(
        &ctx.accounts.midlife_checkpoint.to_account_info(),
        ctx.accounts.requester.key(),
        ctx.accounts.policy_header.key(),
        ctx.accounts.product_terms.key(),
        prepared_slot,
        expires_at_slot,
        &prepared.inputs,
        stop_coupon_index,
    )?;

    Ok(MidlifeNavCheckpointPreview {
        next_coupon_index: view.next_coupon_index,
        final_coupon_index: view.final_coupon_index,
        prepared_slot,
        expires_at_slot,
        sigma_pricing_s6: prepared.inputs.sigma_common_s6,
        now_trading_day: prepared.inputs.now_trading_day,
    })
}
