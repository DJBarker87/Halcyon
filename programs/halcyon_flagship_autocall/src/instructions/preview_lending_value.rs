use anchor_lang::prelude::*;
use halcyon_common::seeds;
use halcyon_kernel::state::{PolicyHeader, PolicyStatus, ProtocolConfig, Regression, VaultSigma};
use halcyon_kernel::KernelError;

use crate::buyback_math::{
    lending_value_payout_usdc, lending_value_s6 as compute_lending_value_s6,
};
use crate::errors::FlagshipAutocallError;
use crate::midlife_pricing;
use crate::state::FlagshipAutocallTerms;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct LendingValuePreview {
    pub nav_s6: i64,
    pub ki_level_usd_s6: i64,
    pub lending_value_s6: i64,
    pub lending_value_payout_usdc: u64,
    pub remaining_coupon_pv_s6: i64,
    pub par_recovery_probability_s6: i64,
    pub sigma_pricing_s6: i64,
    pub now_trading_day: u16,
}

#[derive(Accounts)]
pub struct PreviewLendingValue<'info> {
    #[account(seeds = [seeds::PROTOCOL_CONFIG], seeds::program = halcyon_kernel::ID, bump)]
    pub protocol_config: Account<'info, ProtocolConfig>,
    #[account(
        seeds = [seeds::VAULT_SIGMA, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = vault_sigma.product_program_id == crate::ID @ KernelError::ProductProgramMismatch,
    )]
    pub vault_sigma: Account<'info, VaultSigma>,
    #[account(seeds = [seeds::REGRESSION], seeds::program = halcyon_kernel::ID, bump)]
    pub regression: Account<'info, Regression>,
    #[account(
        constraint = policy_header.product_program_id == crate::ID @ KernelError::ProductProgramMismatch,
        constraint = policy_header.product_terms == product_terms.key() @ FlagshipAutocallError::PolicyStateInvalid,
    )]
    pub policy_header: Account<'info, PolicyHeader>,
    #[account(
        constraint = product_terms.policy_header == policy_header.key() @ FlagshipAutocallError::PolicyStateInvalid,
    )]
    pub product_terms: Account<'info, FlagshipAutocallTerms>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_spy: UncheckedAccount<'info>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_qqq: UncheckedAccount<'info>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_iwm: UncheckedAccount<'info>,
    pub clock: Sysvar<'info, Clock>,
}

pub fn handler(ctx: Context<PreviewLendingValue>) -> Result<LendingValuePreview> {
    require!(
        ctx.accounts.policy_header.status == PolicyStatus::Active,
        FlagshipAutocallError::MidlifeNavUnavailable
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
    let lending_value_s6 =
        compute_lending_value_s6(valuation.nav.nav_s6, valuation.nav.ki_level_usd_s6);
    let lending_value_payout_usdc =
        lending_value_payout_usdc(ctx.accounts.policy_header.notional, lending_value_s6)?;

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
