//! `settle` - manual maturity fallback for SOL Autocall.
//!
//! This is intentionally narrower than `record_observation`: it handles the
//! maturity observation only after all earlier observation windows have
//! already been processed and paid. Terminal settlement therefore includes
//! only the final unpaid coupon, not the historical coupon stream.

use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::get_associated_token_address,
    token::{Mint, Token, TokenAccount},
};
use halcyon_common::{seeds, HalcyonError};
use halcyon_kernel::{
    cpi::accounts::{ApplySettlement, PayCoupon},
    state::{
        CouponVault, PolicyHeader, PolicyStatus, ProductRegistryEntry, ProtocolConfig, VaultState,
    },
    ApplySettlementArgs, KernelError, PayCouponArgs, SettlementReason,
};

use crate::errors::SolAutocallError;
use crate::pricing::coupon_per_observation_usdc;
use crate::state::{ProductStatus, SolAutocallTerms, OBSERVATION_COUNT};

#[derive(Accounts)]
pub struct Settle<'info> {
    pub caller: Signer<'info>,

    #[account(mut)]
    pub policy_header: Box<Account<'info, PolicyHeader>>,

    #[account(
        mut,
        constraint = product_terms.policy_header == policy_header.key() @ SolAutocallError::PolicyStateInvalid,
    )]
    pub product_terms: Box<Account<'info, SolAutocallTerms>>,

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
    pub pyth_sol: UncheckedAccount<'info>,

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
        seeds = [seeds::VAULT_USDC, usdc_mint.key().as_ref()],
        bump,
        constraint = vault_usdc.mint == usdc_mint.key(),
    )]
    pub vault_usdc: Box<Account<'info, TokenAccount>>,

    /// CHECK: kernel PDA authority for `vault_usdc`.
    #[account(seeds = [seeds::VAULT_AUTHORITY], bump)]
    pub vault_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        constraint = buyer_usdc.mint == usdc_mint.key(),
        constraint = buyer_usdc.owner == policy_header.owner @ HalcyonError::ProductAuthorityMismatch,
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

pub fn handler(ctx: Context<Settle>) -> Result<()> {
    // Idempotent no-op once the policy has already been settled.
    if ctx.accounts.policy_header.status != PolicyStatus::Active
        || ctx.accounts.product_terms.status != ProductStatus::Active
    {
        return Ok(());
    }

    require_keys_eq!(
        ctx.accounts.product_registry_entry.product_program_id,
        crate::ID,
        KernelError::ProductProgramMismatch
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

    let final_index = (OBSERVATION_COUNT - 1) as u8;
    require!(
        ctx.accounts.product_terms.current_observation_index == final_index,
        SolAutocallError::PolicyStateInvalid
    );

    let now = ctx.accounts.clock.unix_timestamp;
    let expiry_ts = ctx.accounts.product_terms.observation_schedule[final_index as usize];
    require!(now >= expiry_ts, SolAutocallError::PolicyNotExpired);

    let pyth = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_sol.to_account_info(),
        &halcyon_oracles::feed_ids::SOL_USD,
        &crate::ID,
        &ctx.accounts.clock,
        ctx.accounts.protocol_config.pyth_settle_staleness_cap_secs,
    )?;

    let (principal_payout, final_coupon_usdc) = {
        let terms = &mut ctx.accounts.product_terms;
        let final_coupon_usdc = if pyth.price_s6 >= terms.coupon_barrier_s6 {
            coupon_per_observation_usdc(
                ctx.accounts.policy_header.notional,
                terms.offered_coupon_bps_s6,
            )?
        } else {
            0
        };
        let accumulated_coupon_usdc = terms
            .accumulated_coupon_usdc
            .checked_add(final_coupon_usdc)
            .ok_or(HalcyonError::Overflow)?;
        if pyth.price_s6 <= terms.ki_barrier_s6 {
            terms.ki_triggered = true;
        }

        let principal_payout = if terms.ki_triggered && pyth.price_s6 < terms.entry_price_s6 {
            let recovered = (ctx.accounts.policy_header.notional as u128)
                .checked_mul(pyth.price_s6.max(0) as u128)
                .and_then(|v| v.checked_div(terms.entry_price_s6 as u128))
                .ok_or(HalcyonError::Overflow)?;
            u64::try_from(recovered).map_err(|_| error!(HalcyonError::Overflow))?
        } else {
            ctx.accounts.policy_header.notional
        };

        terms.accumulated_coupon_usdc = accumulated_coupon_usdc;
        terms.current_observation_index = final_index.saturating_add(1);
        terms.status = ProductStatus::Settled;
        (principal_payout, final_coupon_usdc)
    };

    let reason = if pyth.price_s6 <= ctx.accounts.product_terms.ki_barrier_s6 {
        SettlementReason::KnockIn
    } else {
        SettlementReason::Expiry
    };

    require!(
        !ctx.accounts.protocol_config.settlement_paused_global,
        HalcyonError::SettlementPausedGlobally
    );

    if final_coupon_usdc > 0 {
        pay_coupon(&ctx, final_coupon_usdc)?;
    }

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
                buyer_usdc: ctx.accounts.buyer_usdc.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            },
            signer_seeds,
        ),
        ApplySettlementArgs {
            payout: principal_payout,
            reason,
        },
    )
}

fn pay_coupon(ctx: &Context<Settle>, amount: u64) -> Result<()> {
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
        PayCouponArgs { amount },
    )
}
