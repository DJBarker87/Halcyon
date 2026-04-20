use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::get_associated_token_address,
    token::{Mint, Token, TokenAccount},
};
use halcyon_common::{seeds, HalcyonError};
use halcyon_kernel::{
    cpi::accounts::{ApplySettlement, PayCoupon},
    state::{
        CouponVault, KeeperRegistry, PolicyHeader, PolicyStatus, ProductRegistryEntry,
        ProtocolConfig, VaultState,
    },
    ApplySettlementArgs, KernelError, PayCouponArgs, SettlementReason,
};

use crate::errors::FlagshipAutocallError;
use crate::pricing::{
    coupon_due_with_memory_usdc, quarterly_coupon_index, ratio_meets_barrier,
    require_correction_tables_match, worst_ratio_s6,
};
use crate::state::{FlagshipAutocallTerms, ProductStatus, QUARTERLY_AUTOCALL_COUNT};

#[derive(Accounts)]
pub struct RecordAutocallObservation<'info> {
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

pub fn handler(ctx: Context<RecordAutocallObservation>, expected_index: u8) -> Result<()> {
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
        (expected_index as usize) < QUARTERLY_AUTOCALL_COUNT,
        FlagshipAutocallError::ObservationIndexOutOfRange
    );

    if ctx.accounts.policy_header.status != PolicyStatus::Active
        || ctx.accounts.product_terms.status != ProductStatus::Active
    {
        return Ok(());
    }
    require_correction_tables_match(&ctx.accounts.protocol_config)?;

    let terms = &ctx.accounts.product_terms;
    if terms.next_autocall_index > expected_index {
        return Ok(());
    }
    require!(
        terms.next_autocall_index == expected_index,
        FlagshipAutocallError::ObservationIndexOutOfRange
    );

    let now = ctx.accounts.clock.unix_timestamp;
    let scheduled_ts = terms.quarterly_autocall_schedule[expected_index as usize];
    require!(
        now >= scheduled_ts,
        FlagshipAutocallError::ObservationNotDue
    );

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
    let should_autocall =
        ratio_meets_barrier(worst_ratio, ctx.accounts.product_terms.autocall_barrier_bps)?;

    let coupon_index = quarterly_coupon_index(expected_index)?;
    if ctx.accounts.product_terms.next_coupon_index == coupon_index {
        let coupon_scheduled_ts =
            ctx.accounts.product_terms.monthly_coupon_schedule[coupon_index as usize];
        if now >= coupon_scheduled_ts
            && ratio_meets_barrier(worst_ratio, ctx.accounts.product_terms.coupon_barrier_bps)?
        {
            let coupon_due = coupon_due_with_memory_usdc(
                ctx.accounts.policy_header.notional,
                ctx.accounts.product_terms.offered_coupon_bps_s6,
                ctx.accounts.product_terms.missed_coupon_observations,
            )?;
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
                let terms = &mut ctx.accounts.product_terms;
                terms.coupons_paid_usdc = terms
                    .coupons_paid_usdc
                    .checked_add(coupon_due)
                    .ok_or(HalcyonError::Overflow)?;
                terms.missed_coupon_observations = 0;
                terms.next_coupon_index = coupon_index.saturating_add(1);
            }
        }
    }

    {
        let terms = &mut ctx.accounts.product_terms;
        terms.next_autocall_index = expected_index.saturating_add(1);
    }

    if !should_autocall {
        return Ok(());
    }
    require!(
        !ctx.accounts.protocol_config.settlement_paused_global,
        HalcyonError::SettlementPausedGlobally
    );

    let bump = ctx.bumps.product_authority;
    let signer_seeds: &[&[&[u8]]] = &[&[seeds::PRODUCT_AUTHORITY, &[bump]]];
    {
        let terms = &mut ctx.accounts.product_terms;
        terms.settled_payout_usdc = ctx.accounts.policy_header.notional;
        terms.settled_at = now;
        terms.status = ProductStatus::AutoCalled;
    }
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
            payout: ctx.accounts.policy_header.notional,
            reason: SettlementReason::Autocall,
        },
    )
}
