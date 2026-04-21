use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::get_associated_token_address,
    token::{Mint, Token, TokenAccount},
};
use halcyon_common::{seeds, HalcyonError};
use halcyon_kernel::{
    cpi::accounts::PayCoupon,
    state::{
        CouponVault, KeeperRegistry, PolicyHeader, PolicyStatus, ProductRegistryEntry,
        ProtocolConfig, VaultState,
    },
    KernelError, PayCouponArgs,
};

use crate::errors::FlagshipAutocallError;
use crate::observation::{
    commit_coupon_observation, coupon_outcome, read_equity_observation_worst_ratio_s6,
};
use crate::pricing::require_correction_tables_match;
use crate::state::{FlagshipAutocallTerms, ProductStatus, MONTHLY_COUPON_COUNT};

#[derive(Accounts)]
pub struct RecordCouponObservation<'info> {
    pub keeper: Signer<'info>,

    #[account(seeds = [seeds::KEEPER_REGISTRY], bump)]
    pub keeper_registry: Box<Account<'info, KeeperRegistry>>,

    #[account(mut)]
    pub policy_header: Box<Account<'info, PolicyHeader>>,

    #[account(
        mut,
        constraint = product_terms.policy_header == policy_header.key() @ FlagshipAutocallError::PolicyStateInvalid,
    )]
    pub product_terms: Box<Account<'info, FlagshipAutocallTerms>>,

    #[account(
        mut,
        seeds = [seeds::PRODUCT_REGISTRY, crate::ID.as_ref()],
        bump,
        constraint = product_registry_entry.product_program_id == crate::ID
            @ KernelError::ProductProgramMismatch,
    )]
    pub product_registry_entry: Box<Account<'info, ProductRegistryEntry>>,

    #[account(seeds = [seeds::PROTOCOL_CONFIG], bump)]
    pub protocol_config: Box<Account<'info, ProtocolConfig>>,

    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_spy: UncheckedAccount<'info>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_qqq: UncheckedAccount<'info>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_iwm: UncheckedAccount<'info>,

    pub usdc_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        seeds = [seeds::COUPON_VAULT, crate::ID.as_ref()],
        bump,
        constraint = coupon_vault.product_program_id == product_registry_entry.product_program_id
            @ KernelError::ProductProgramMismatch,
    )]
    pub coupon_vault: Box<Account<'info, CouponVault>>,

    #[account(
        mut,
        constraint = coupon_vault_usdc.mint == usdc_mint.key(),
        constraint = coupon_vault_usdc.owner == coupon_vault.key() @ KernelError::ProductProgramMismatch,
        constraint = coupon_vault_usdc.key()
            == get_associated_token_address(&coupon_vault.key(), &usdc_mint.key())
            @ KernelError::ProductProgramMismatch,
    )]
    pub coupon_vault_usdc: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = buyer_usdc.mint == usdc_mint.key(),
        constraint = buyer_usdc.owner == policy_header.owner @ HalcyonError::ProductAuthorityMismatch,
        constraint = buyer_usdc.key()
            == get_associated_token_address(&policy_header.owner, &usdc_mint.key())
            @ HalcyonError::ProductAuthorityMismatch,
    )]
    pub buyer_usdc: Box<Account<'info, TokenAccount>>,

    /// CHECK: canonical PDA signer for kernel CPIs.
    #[account(seeds = [seeds::PRODUCT_AUTHORITY], bump)]
    pub product_authority: UncheckedAccount<'info>,

    #[account(mut, seeds = [seeds::VAULT_STATE], bump)]
    pub vault_state: Box<Account<'info, VaultState>>,

    pub clock: Sysvar<'info, Clock>,
    pub kernel_program: Program<'info, halcyon_kernel::program::HalcyonKernel>,
    pub token_program: Program<'info, Token>,
}

pub fn handler(ctx: Context<RecordCouponObservation>, expected_index: u8) -> Result<()> {
    require_keys_eq!(
        ctx.accounts.keeper.key(),
        ctx.accounts.keeper_registry.observation,
        HalcyonError::KeeperAuthorityMismatch
    );
    require_keys_eq!(
        ctx.accounts.policy_header.product_program_id,
        ctx.accounts.product_registry_entry.product_program_id,
        KernelError::ProductProgramMismatch
    );
    require_keys_eq!(
        ctx.accounts.product_registry_entry.expected_authority,
        ctx.accounts.product_authority.key(),
        HalcyonError::ProductAuthorityMismatch
    );
    require!(
        (expected_index as usize) < MONTHLY_COUPON_COUNT,
        FlagshipAutocallError::ObservationIndexOutOfRange
    );

    if ctx.accounts.policy_header.status != PolicyStatus::Active
        || ctx.accounts.product_terms.status != ProductStatus::Active
    {
        return Ok(());
    }
    require!(
        !ctx.accounts.product_registry_entry.paused,
        HalcyonError::IssuancePausedPerProduct
    );
    require_correction_tables_match(&ctx.accounts.protocol_config)?;

    let terms = &ctx.accounts.product_terms;
    if terms.next_coupon_index > expected_index {
        return Ok(());
    }
    require!(
        terms.next_coupon_index == expected_index,
        FlagshipAutocallError::ObservationIndexOutOfRange
    );

    let now = ctx.accounts.clock.unix_timestamp;
    let scheduled_ts = terms.monthly_coupon_schedule[expected_index as usize];
    require!(
        now >= scheduled_ts,
        FlagshipAutocallError::ObservationNotDue
    );

    let worst_ratio = read_equity_observation_worst_ratio_s6(
        &ctx.accounts.product_terms,
        scheduled_ts,
        &ctx.accounts.pyth_spy.to_account_info(),
        &ctx.accounts.pyth_qqq.to_account_info(),
        &ctx.accounts.pyth_iwm.to_account_info(),
    )?;
    let (should_pay, coupon_due) = coupon_outcome(
        &ctx.accounts.policy_header,
        &ctx.accounts.product_terms,
        worst_ratio,
    )?;

    if coupon_due > 0 {
        let bump = ctx.bumps.product_authority;
        let signer_seeds: &[&[&[u8]]] = &[&[seeds::PRODUCT_AUTHORITY, &[bump]]];
        halcyon_kernel::cpi::pay_coupon(
            CpiContext::new_with_signer(
                ctx.accounts.kernel_program.to_account_info(),
                PayCoupon {
                    product_authority: ctx.accounts.product_authority.to_account_info(),
                    product_registry_entry: ctx.accounts.product_registry_entry.to_account_info(),
                    protocol_config: ctx.accounts.protocol_config.to_account_info(),
                    vault_state: ctx.accounts.vault_state.to_account_info(),
                    policy_header: ctx.accounts.policy_header.to_account_info(),
                    usdc_mint: ctx.accounts.usdc_mint.to_account_info(),
                    coupon_vault: ctx.accounts.coupon_vault.to_account_info(),
                    coupon_vault_usdc: ctx.accounts.coupon_vault_usdc.to_account_info(),
                    buyer_usdc: ctx.accounts.buyer_usdc.to_account_info(),
                    token_program: ctx.accounts.token_program.to_account_info(),
                },
                signer_seeds,
            ),
            PayCouponArgs { amount: coupon_due },
        )?;
    }

    commit_coupon_observation(
        &mut ctx.accounts.product_terms,
        expected_index,
        should_pay,
        coupon_due,
    )
}
