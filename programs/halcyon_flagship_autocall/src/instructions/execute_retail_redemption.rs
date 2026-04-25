use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::get_associated_token_address,
    token::{Mint, Token, TokenAccount},
};
use halcyon_common::{seeds, HalcyonError};
use halcyon_kernel::{
    cpi::accounts::ApplySettlement,
    state::{
        PolicyHeader, PolicyStatus, ProductRegistryEntry, ProtocolConfig, Regression, VaultSigma,
        VaultState,
    },
    ApplySettlementArgs, KernelError, SettlementReason,
};

use crate::buyback_math::{
    lending_value_payout_usdc, retail_redemption_value_s6 as compute_retail_redemption_value_s6,
};
use crate::errors::FlagshipAutocallError;
use crate::midlife_pricing;
use crate::state::{FlagshipAutocallTerms, ProductStatus, RetailRedemptionRequest};

#[event]
pub struct FlagshipRetailRedemptionExecuted {
    pub policy_id: Pubkey,
    pub owner: Pubkey,
    pub nav_s6: i64,
    pub ki_level_s6: i64,
    pub redemption_value_s6: i64,
    pub payout_usdc: u64,
    pub requested_at: i64,
    pub executed_at: i64,
}

#[derive(Accounts)]
pub struct ExecuteRetailRedemption<'info> {
    pub policy_owner: Signer<'info>,

    #[account(
        mut,
        constraint = policy_header.product_program_id == crate::ID @ KernelError::ProductProgramMismatch,
        constraint = policy_header.owner == policy_owner.key() @ HalcyonError::ProductAuthorityMismatch,
        constraint = policy_header.product_terms == product_terms.key() @ FlagshipAutocallError::PolicyStateInvalid,
    )]
    pub policy_header: Box<Account<'info, PolicyHeader>>,

    #[account(
        mut,
        constraint = product_terms.policy_header == policy_header.key() @ FlagshipAutocallError::PolicyStateInvalid,
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

    #[account(
        mut,
        seeds = [seeds::PRODUCT_REGISTRY, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = product_registry_entry.product_program_id == crate::ID
            @ KernelError::ProductProgramMismatch,
    )]
    pub product_registry_entry: Box<Account<'info, ProductRegistryEntry>>,

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

    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_spy: UncheckedAccount<'info>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_qqq: UncheckedAccount<'info>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_iwm: UncheckedAccount<'info>,

    pub usdc_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        seeds = [seeds::VAULT_USDC, usdc_mint.key().as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = vault_usdc.mint == usdc_mint.key(),
    )]
    pub vault_usdc: Box<Account<'info, TokenAccount>>,

    /// CHECK: kernel PDA authority for `vault_usdc`.
    #[account(seeds = [seeds::VAULT_AUTHORITY], seeds::program = halcyon_kernel::ID, bump)]
    pub vault_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        constraint = owner_usdc.mint == usdc_mint.key(),
        constraint = owner_usdc.owner == policy_header.owner @ HalcyonError::ProductAuthorityMismatch,
        constraint = owner_usdc.key()
            == get_associated_token_address(&policy_header.owner, &usdc_mint.key())
            @ HalcyonError::ProductAuthorityMismatch,
    )]
    pub owner_usdc: Box<Account<'info, TokenAccount>>,

    /// CHECK: canonical PDA signer for kernel CPIs.
    #[account(seeds = [seeds::PRODUCT_AUTHORITY], bump)]
    pub product_authority: UncheckedAccount<'info>,

    #[account(mut, seeds = [seeds::VAULT_STATE], seeds::program = halcyon_kernel::ID, bump)]
    pub vault_state: Box<Account<'info, VaultState>>,

    pub clock: Sysvar<'info, Clock>,
    pub kernel_program: Program<'info, halcyon_kernel::program::HalcyonKernel>,
    pub token_program: Program<'info, Token>,
}

pub fn handler(ctx: Context<ExecuteRetailRedemption>) -> Result<()> {
    require_keys_eq!(
        ctx.accounts.product_registry_entry.expected_authority,
        ctx.accounts.product_authority.key(),
        HalcyonError::ProductAuthorityMismatch
    );
    require!(
        !ctx.accounts.product_registry_entry.paused,
        HalcyonError::IssuancePausedPerProduct
    );
    require!(
        ctx.accounts.policy_header.status == PolicyStatus::Active
            && ctx.accounts.product_terms.status == ProductStatus::Active,
        FlagshipAutocallError::PolicyStateInvalid
    );

    let now = ctx.accounts.clock.unix_timestamp;
    require!(
        now >= ctx.accounts.redemption_request.earliest_execute_ts,
        FlagshipAutocallError::RetailRedemptionNotReady
    );
    require!(
        now <= ctx.accounts.redemption_request.expires_at,
        FlagshipAutocallError::RetailRedemptionExpired
    );

    let valuation = midlife_pricing::compute_nav_from_accounts(
        &ctx.accounts.protocol_config,
        &ctx.accounts.vault_sigma,
        &ctx.accounts.regression,
        &ctx.accounts.policy_header,
        &ctx.accounts.product_terms,
        &ctx.accounts.pyth_spy.to_account_info(),
        &ctx.accounts.pyth_qqq.to_account_info(),
        &ctx.accounts.pyth_iwm.to_account_info(),
        &ctx.accounts.clock,
    )?;
    let redemption_value_s6 =
        compute_retail_redemption_value_s6(valuation.nav.nav_s6, valuation.nav.ki_level_usd_s6);
    let payout =
        lending_value_payout_usdc(ctx.accounts.policy_header.notional, redemption_value_s6)?;

    ctx.accounts.product_terms.settled_payout_usdc = payout;
    ctx.accounts.product_terms.settled_at = now;
    ctx.accounts.product_terms.status = ProductStatus::Settled;

    let bump = ctx.bumps.product_authority;
    let signer_seeds: &[&[&[u8]]] = &[&[seeds::PRODUCT_AUTHORITY, &[bump]]];
    halcyon_kernel::cpi::apply_settlement(
        CpiContext::new_with_signer(
            ctx.accounts.kernel_program.to_account_info(),
            ApplySettlement {
                product_authority: ctx.accounts.product_authority.to_account_info(),
                product_registry_entry: ctx.accounts.product_registry_entry.to_account_info(),
                protocol_config: ctx.accounts.protocol_config.to_account_info(),
                vault_state: ctx.accounts.vault_state.to_account_info(),
                policy_header: ctx.accounts.policy_header.to_account_info(),
                usdc_mint: ctx.accounts.usdc_mint.to_account_info(),
                vault_usdc: ctx.accounts.vault_usdc.to_account_info(),
                vault_authority: ctx.accounts.vault_authority.to_account_info(),
                buyer_usdc: ctx.accounts.owner_usdc.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            },
            signer_seeds,
        ),
        ApplySettlementArgs {
            payout,
            reason: SettlementReason::RetailRedemption,
        },
    )?;

    emit!(FlagshipRetailRedemptionExecuted {
        policy_id: ctx.accounts.policy_header.key(),
        owner: ctx.accounts.policy_owner.key(),
        nav_s6: valuation.nav.nav_s6,
        ki_level_s6: valuation.nav.ki_level_usd_s6,
        redemption_value_s6,
        payout_usdc: payout,
        requested_at: ctx.accounts.redemption_request.requested_at,
        executed_at: now,
    });

    Ok(())
}
