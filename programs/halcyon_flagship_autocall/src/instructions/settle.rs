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

use crate::errors::FlagshipAutocallError;
use crate::pricing::{
    coupon_due_with_memory_usdc, maturity_payout_usdc, ratio_meets_barrier,
    require_correction_tables_match, worst_ratio_s6,
};
use crate::state::{FlagshipAutocallTerms, ProductStatus};

#[derive(Accounts)]
pub struct Settle<'info> {
    pub caller: Signer<'info>,

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

pub fn handler(ctx: Context<Settle>) -> Result<()> {
    if ctx.accounts.policy_header.status != PolicyStatus::Active
        || ctx.accounts.product_terms.status != ProductStatus::Active
    {
        return Ok(());
    }
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

    let now = ctx.accounts.clock.unix_timestamp;
    require!(
        now >= ctx.accounts.policy_header.expiry_ts,
        FlagshipAutocallError::PolicyNotExpired
    );
    require_correction_tables_match(&ctx.accounts.protocol_config)?;

    let spy = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_spy.to_account_info(),
        &halcyon_oracles::feed_ids::SPY_USD,
        &crate::ID,
        &ctx.accounts.clock,
        ctx.accounts.protocol_config.pyth_settle_staleness_cap_secs,
    )?;
    let qqq = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_qqq.to_account_info(),
        &halcyon_oracles::feed_ids::QQQ_USD,
        &crate::ID,
        &ctx.accounts.clock,
        ctx.accounts.protocol_config.pyth_settle_staleness_cap_secs,
    )?;
    let iwm = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_iwm.to_account_info(),
        &halcyon_oracles::feed_ids::IWM_USD,
        &crate::ID,
        &ctx.accounts.clock,
        ctx.accounts.protocol_config.pyth_settle_staleness_cap_secs,
    )?;
    let worst_ratio = worst_ratio_s6(
        &ctx.accounts.product_terms,
        spy.price_s6,
        qqq.price_s6,
        iwm.price_s6,
    )?;

    if (ctx.accounts.product_terms.next_coupon_index as usize)
        < ctx.accounts.product_terms.monthly_coupon_schedule.len()
    {
        let next_coupon_idx = ctx.accounts.product_terms.next_coupon_index as usize;
        let coupon_due_ts = ctx.accounts.product_terms.monthly_coupon_schedule[next_coupon_idx];
        if now >= coupon_due_ts {
            let coupon_due =
                if ratio_meets_barrier(worst_ratio, ctx.accounts.product_terms.coupon_barrier_bps)?
                {
                    coupon_due_with_memory_usdc(
                        ctx.accounts.policy_header.notional,
                        ctx.accounts.product_terms.offered_coupon_bps_s6,
                        ctx.accounts.product_terms.missed_coupon_observations,
                    )?
                } else {
                    0
                };
            if coupon_due > 0 {
                let bump = ctx.bumps.product_authority;
                let signer_seeds: &[&[&[u8]]] = &[&[seeds::PRODUCT_AUTHORITY, &[bump]]];
                halcyon_kernel::cpi::pay_coupon(
                    CpiContext::new_with_signer(
                        ctx.accounts.kernel_program.to_account_info(),
                        PayCoupon {
                            product_authority: ctx.accounts.product_authority.to_account_info(),
                            product_registry_entry: ctx
                                .accounts
                                .product_registry_entry
                                .to_account_info(),
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
                ctx.accounts.product_terms.coupons_paid_usdc = ctx
                    .accounts
                    .product_terms
                    .coupons_paid_usdc
                    .checked_add(coupon_due)
                    .ok_or(HalcyonError::Overflow)?;
            }
            ctx.accounts.product_terms.missed_coupon_observations = 0;
            ctx.accounts.product_terms.next_coupon_index = ctx
                .accounts
                .product_terms
                .next_coupon_index
                .saturating_add(1);
        }
    }

    let payout = maturity_payout_usdc(
        &ctx.accounts.policy_header,
        &ctx.accounts.product_terms,
        worst_ratio,
    )?;

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
                buyer_usdc: ctx.accounts.buyer_usdc.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            },
            signer_seeds,
        ),
        ApplySettlementArgs {
            payout,
            reason: if ctx.accounts.product_terms.ki_latched {
                SettlementReason::KnockIn
            } else {
                SettlementReason::Expiry
            },
        },
    )
}
