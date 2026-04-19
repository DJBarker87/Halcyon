//! `record_observation` - keeper-driven SOL Autocall observation handler.
//!
//! Each observation either:
//!   * pays the scheduled coupon immediately through `kernel::pay_coupon`, or
//!   * settles the note if autocall / maturity conditions fire.
//! Prior coupons are therefore never re-paid at terminal settlement.

use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use halcyon_common::{seeds, HalcyonError};
use halcyon_kernel::{
    cpi::accounts::{ApplySettlement, PayCoupon},
    state::{
        CouponVault, KeeperRegistry, PolicyHeader, PolicyStatus, ProductRegistryEntry,
        ProtocolConfig, VaultState,
    },
    ApplySettlementArgs, KernelError, PayCouponArgs, SettlementReason,
};

use crate::errors::SolAutocallError;
use crate::pricing::coupon_per_observation_usdc;
use crate::state::{ProductStatus, SolAutocallTerms, OBSERVATION_COUNT};

#[event]
pub struct ObservationRecorded {
    pub policy_id: Pubkey,
    pub observation_index: u8,
    pub price_s6: i64,
    pub coupon_accrued_usdc: u64,
    pub ki_triggered: bool,
    pub recorded_at: i64,
}

#[event]
pub struct PolicyAutoCalled {
    pub policy_id: Pubkey,
    pub observation_index: u8,
    pub payout_usdc: u64,
    pub recorded_at: i64,
}

#[derive(Accounts)]
pub struct RecordObservation<'info> {
    pub keeper: Signer<'info>,

    #[account(seeds = [seeds::KEEPER_REGISTRY], bump)]
    pub keeper_registry: Box<Account<'info, KeeperRegistry>>,

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

pub fn handler(ctx: Context<RecordObservation>, expected_index: u8) -> Result<()> {
    require_keys_eq!(
        ctx.accounts.keeper.key(),
        ctx.accounts.keeper_registry.observation,
        HalcyonError::KeeperAuthorityMismatch
    );
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

    require!(
        (expected_index as usize) < OBSERVATION_COUNT,
        SolAutocallError::ObservationIndexOutOfRange
    );

    // Idempotent no-op for replays after the policy has already moved on.
    if ctx.accounts.policy_header.status != PolicyStatus::Active
        || ctx.accounts.product_terms.status != ProductStatus::Active
    {
        return Ok(());
    }
    require!(
        !ctx.accounts.product_registry_entry.paused,
        HalcyonError::IssuancePausedPerProduct
    );

    let terms = &ctx.accounts.product_terms;
    if terms.current_observation_index > expected_index {
        return Ok(());
    }
    require!(
        terms.current_observation_index == expected_index,
        SolAutocallError::ObservationIndexOutOfRange
    );

    let now = ctx.accounts.clock.unix_timestamp;
    let scheduled_ts = terms.observation_schedule[expected_index as usize];
    require!(now >= scheduled_ts, SolAutocallError::ObservationNotDue);

    let pyth = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_sol.to_account_info(),
        &halcyon_oracles::feed_ids::SOL_USD,
        &crate::ID,
        &ctx.accounts.clock,
        ctx.accounts.protocol_config.pyth_settle_staleness_cap_secs,
    )?;

    let coupon_accrued_usdc =
        coupon_to_accrue(ctx.accounts.policy_header.notional, terms, pyth.price_s6)?;
    let accumulated_coupon_usdc = terms
        .accumulated_coupon_usdc
        .checked_add(coupon_accrued_usdc)
        .ok_or(HalcyonError::Overflow)?;
    let ki_triggered = terms.ki_triggered || pyth.price_s6 <= terms.ki_barrier_s6;

    emit!(ObservationRecorded {
        policy_id: ctx.accounts.policy_header.key(),
        observation_index: expected_index,
        price_s6: pyth.price_s6,
        coupon_accrued_usdc,
        ki_triggered,
        recorded_at: now,
    });

    let is_autocall_allowed = expected_index >= terms.no_autocall_first_n_obs;
    let is_autocall = is_autocall_allowed && pyth.price_s6 >= terms.autocall_barrier_s6;
    let is_final_observation = terms.is_final_observation(expected_index);

    if is_autocall {
        require!(
            !ctx.accounts.protocol_config.settlement_paused_global,
            HalcyonError::SettlementPausedGlobally
        );
        let principal_payout = ctx.accounts.policy_header.notional;
        let payout = principal_payout
            .checked_add(coupon_accrued_usdc)
            .ok_or(HalcyonError::Overflow)?;
        {
            let terms = &mut ctx.accounts.product_terms;
            terms.accumulated_coupon_usdc = accumulated_coupon_usdc;
            terms.current_observation_index = expected_index.saturating_add(1);
            terms.status = ProductStatus::AutoCalled;
        }
        if coupon_accrued_usdc > 0 {
            pay_coupon(&ctx, coupon_accrued_usdc)?;
        }
        apply_settlement(&ctx, principal_payout, SettlementReason::Autocall)?;
        emit!(PolicyAutoCalled {
            policy_id: ctx.accounts.policy_header.key(),
            observation_index: expected_index,
            payout_usdc: payout,
            recorded_at: now,
        });
        return Ok(());
    }

    if is_final_observation {
        require!(
            !ctx.accounts.protocol_config.settlement_paused_global,
            HalcyonError::SettlementPausedGlobally
        );
        let principal_payout = maturity_principal_payout_usdc(
            ctx.accounts.policy_header.notional,
            ki_triggered,
            terms.entry_price_s6,
            pyth.price_s6,
        )?;
        {
            let terms = &mut ctx.accounts.product_terms;
            terms.accumulated_coupon_usdc = accumulated_coupon_usdc;
            terms.current_observation_index = expected_index.saturating_add(1);
            terms.status = ProductStatus::Settled;
            terms.ki_triggered = ki_triggered;
        }
        let reason = if ki_triggered {
            SettlementReason::KnockIn
        } else {
            SettlementReason::Expiry
        };
        if coupon_accrued_usdc > 0 {
            pay_coupon(&ctx, coupon_accrued_usdc)?;
        }
        apply_settlement(&ctx, principal_payout, reason)?;
        return Ok(());
    }

    if coupon_accrued_usdc > 0 {
        pay_coupon(&ctx, coupon_accrued_usdc)?;
    }

    {
        let terms = &mut ctx.accounts.product_terms;
        terms.accumulated_coupon_usdc = accumulated_coupon_usdc;
        terms.current_observation_index = expected_index.saturating_add(1);
        terms.ki_triggered = ki_triggered;
    }
    Ok(())
}

fn coupon_to_accrue(
    notional_usdc: u64,
    terms: &SolAutocallTerms,
    current_price_s6: i64,
) -> Result<u64> {
    if current_price_s6 < terms.coupon_barrier_s6 {
        return Ok(0);
    }
    coupon_per_observation_usdc(notional_usdc, terms.offered_coupon_bps_s6)
}

fn maturity_principal_payout_usdc(
    notional_usdc: u64,
    ki_triggered: bool,
    entry_price_s6: i64,
    current_price_s6: i64,
) -> Result<u64> {
    require!(entry_price_s6 > 0, SolAutocallError::InvalidEntryPrice);
    let principal = if ki_triggered && current_price_s6 < entry_price_s6 {
        let recovered = (notional_usdc as u128)
            .checked_mul(current_price_s6.max(0) as u128)
            .and_then(|v| v.checked_div(entry_price_s6 as u128))
            .ok_or(HalcyonError::Overflow)?;
        u64::try_from(recovered).map_err(|_| error!(HalcyonError::Overflow))?
    } else {
        notional_usdc
    };
    Ok(principal)
}

fn pay_coupon(ctx: &Context<RecordObservation>, amount: u64) -> Result<()> {
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

fn apply_settlement(
    ctx: &Context<RecordObservation>,
    payout: u64,
    reason: SettlementReason,
) -> Result<()> {
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
        ApplySettlementArgs { payout, reason },
    )
}
