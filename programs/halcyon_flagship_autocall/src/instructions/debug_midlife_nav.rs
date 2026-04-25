#![cfg(all(
    any(feature = "integration-test", feature = "idl-build"),
    not(feature = "cpi")
))]

use anchor_lang::prelude::*;
use halcyon_common::HalcyonError;
use halcyon_flagship_quote::midlife_pricer::{
    compute_midlife_nav, MidlifeInputs, MidlifePricerError,
};

use crate::errors::FlagshipAutocallError;
use crate::instructions::prepare_midlife_nav::MidlifeNavCheckpointPreview;
use crate::midlife_pricing;
use crate::state::MIDLIFE_NAV_CHECKPOINT_EXPIRY_SLOTS;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct DebugMidlifeInputs {
    pub current_spy_s6: i64,
    pub current_qqq_s6: i64,
    pub current_iwm_s6: i64,
    pub sigma_common_s6: i64,
    pub entry_spy_s6: i64,
    pub entry_qqq_s6: i64,
    pub entry_iwm_s6: i64,
    pub beta_spy_s12: i128,
    pub beta_qqq_s12: i128,
    pub alpha_s12: i128,
    pub regression_residual_vol_s6: i64,
    pub monthly_coupon_schedule: [i64; 18],
    pub quarterly_autocall_schedule: [i64; 6],
    pub next_coupon_index: u8,
    pub next_autocall_index: u8,
    pub offered_coupon_bps_s6: i64,
    pub coupon_barrier_bps: u16,
    pub autocall_barrier_bps: u16,
    pub ki_barrier_bps: u16,
    pub ki_latched: bool,
    pub missed_coupon_observations: u8,
    pub coupons_paid_usdc: u64,
    pub notional_usdc: u64,
    pub now_trading_day: u16,
}

impl From<DebugMidlifeInputs> for MidlifeInputs {
    fn from(inputs: DebugMidlifeInputs) -> Self {
        Self {
            current_spy_s6: inputs.current_spy_s6,
            current_qqq_s6: inputs.current_qqq_s6,
            current_iwm_s6: inputs.current_iwm_s6,
            sigma_common_s6: inputs.sigma_common_s6,
            entry_spy_s6: inputs.entry_spy_s6,
            entry_qqq_s6: inputs.entry_qqq_s6,
            entry_iwm_s6: inputs.entry_iwm_s6,
            beta_spy_s12: inputs.beta_spy_s12,
            beta_qqq_s12: inputs.beta_qqq_s12,
            alpha_s12: inputs.alpha_s12,
            regression_residual_vol_s6: inputs.regression_residual_vol_s6,
            monthly_coupon_schedule: inputs.monthly_coupon_schedule,
            quarterly_autocall_schedule: inputs.quarterly_autocall_schedule,
            next_coupon_index: inputs.next_coupon_index,
            next_autocall_index: inputs.next_autocall_index,
            offered_coupon_bps_s6: inputs.offered_coupon_bps_s6,
            coupon_barrier_bps: inputs.coupon_barrier_bps,
            autocall_barrier_bps: inputs.autocall_barrier_bps,
            ki_barrier_bps: inputs.ki_barrier_bps,
            ki_latched: inputs.ki_latched,
            missed_coupon_observations: inputs.missed_coupon_observations,
            coupons_paid_usdc: inputs.coupons_paid_usdc,
            notional_usdc: inputs.notional_usdc,
            now_trading_day: inputs.now_trading_day,
        }
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct DebugMidlifeNav {
    pub nav_s6: i64,
    pub ki_level_usd_s6: i64,
    pub remaining_coupon_pv_s6: i64,
    pub par_recovery_probability_s6: i64,
}

#[derive(Accounts)]
pub struct DebugMidlifeNavView {}

#[derive(Accounts)]
pub struct DebugMidlifeNavPrepare<'info> {
    pub requester: Signer<'info>,
    /// CHECK: owned by this program and manually initialized as a fixed-size
    /// debug midlife checkpoint byte account.
    #[account(mut)]
    pub midlife_checkpoint: UncheckedAccount<'info>,
    pub clock: Sysvar<'info, Clock>,
}

#[derive(Accounts)]
pub struct DebugMidlifeNavAdvance<'info> {
    pub requester: Signer<'info>,
    /// CHECK: owned by this program and manually validated as a debug midlife
    /// checkpoint byte account.
    #[account(mut)]
    pub midlife_checkpoint: UncheckedAccount<'info>,
    pub clock: Sysvar<'info, Clock>,
}

#[derive(Accounts)]
pub struct DebugMidlifeNavFinish<'info> {
    #[account(mut)]
    pub requester: Signer<'info>,
    /// CHECK: owned by this program and manually validated/closed as a debug
    /// midlife checkpoint byte account.
    #[account(mut)]
    pub midlife_checkpoint: UncheckedAccount<'info>,
    pub clock: Sysvar<'info, Clock>,
}

fn map_midlife_pricer_error(err: MidlifePricerError) -> Error {
    match err {
        MidlifePricerError::NotImplemented => error!(FlagshipAutocallError::MidlifeNavUnavailable),
        MidlifePricerError::InvalidInput => error!(FlagshipAutocallError::MidlifeNavInvalid),
        MidlifePricerError::MathError => error!(FlagshipAutocallError::MidlifeNavMathFailed),
    }
}

pub fn handler(
    _ctx: Context<DebugMidlifeNavView>,
    inputs: DebugMidlifeInputs,
) -> Result<DebugMidlifeNav> {
    let nav = compute_midlife_nav(&inputs.into()).map_err(map_midlife_pricer_error)?;
    Ok(DebugMidlifeNav {
        nav_s6: nav.nav_s6,
        ki_level_usd_s6: nav.ki_level_usd_s6,
        remaining_coupon_pv_s6: nav.remaining_coupon_pv_s6,
        par_recovery_probability_s6: nav.par_recovery_probability_s6,
    })
}

fn validate_debug_checkpoint(
    checkpoint: &AccountInfo<'_>,
    requester: Pubkey,
    current_slot: u64,
) -> Result<()> {
    midlife_pricing::validate_checkpoint_account(
        checkpoint,
        requester,
        Pubkey::default(),
        Pubkey::default(),
        current_slot,
    )
}

pub fn prepare_handler(
    ctx: Context<DebugMidlifeNavPrepare>,
    inputs: DebugMidlifeInputs,
    stop_coupon_index: u8,
) -> Result<MidlifeNavCheckpointPreview> {
    let inputs: MidlifeInputs = inputs.into();
    let prepared_slot = ctx.accounts.clock.slot;
    let expires_at_slot = prepared_slot
        .checked_add(MIDLIFE_NAV_CHECKPOINT_EXPIRY_SLOTS)
        .ok_or(HalcyonError::Overflow)?;
    let view = midlife_pricing::write_monthly_debug_checkpoint_account_from_inputs(
        &ctx.accounts.midlife_checkpoint.to_account_info(),
        ctx.accounts.requester.key(),
        Pubkey::default(),
        Pubkey::default(),
        prepared_slot,
        expires_at_slot,
        &inputs,
        stop_coupon_index,
    )?;

    Ok(MidlifeNavCheckpointPreview {
        next_coupon_index: view.next_coupon_index,
        final_coupon_index: view.final_coupon_index,
        prepared_slot,
        expires_at_slot,
        sigma_pricing_s6: inputs.sigma_common_s6,
        now_trading_day: inputs.now_trading_day,
    })
}

pub fn advance_handler(
    ctx: Context<DebugMidlifeNavAdvance>,
    stop_coupon_index: u8,
) -> Result<MidlifeNavCheckpointPreview> {
    validate_debug_checkpoint(
        &ctx.accounts.midlife_checkpoint.to_account_info(),
        ctx.accounts.requester.key(),
        ctx.accounts.clock.slot,
    )?;

    let view = midlife_pricing::advance_monthly_debug_checkpoint(
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

pub fn finish_handler(ctx: Context<DebugMidlifeNavFinish>) -> Result<DebugMidlifeNav> {
    validate_debug_checkpoint(
        &ctx.accounts.midlife_checkpoint.to_account_info(),
        ctx.accounts.requester.key(),
        ctx.accounts.clock.slot,
    )?;

    let valuation = midlife_pricing::finish_monthly_debug_nav_from_checkpoint(
        &ctx.accounts.midlife_checkpoint.to_account_info(),
    )?;
    midlife_pricing::close_checkpoint_account(
        &ctx.accounts.midlife_checkpoint.to_account_info(),
        &ctx.accounts.requester.to_account_info(),
    )?;
    Ok(DebugMidlifeNav {
        nav_s6: valuation.nav.nav_s6,
        ki_level_usd_s6: valuation.nav.ki_level_usd_s6,
        remaining_coupon_pv_s6: valuation.nav.remaining_coupon_pv_s6,
        par_recovery_probability_s6: valuation.nav.par_recovery_probability_s6,
    })
}
