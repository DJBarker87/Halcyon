//! `preview_lending_value` - read-only SOL autocall midlife collateral mark.

use anchor_lang::prelude::*;
use halcyon_common::seeds;
use halcyon_kernel::state::{PolicyHeader, PolicyStatus, ProtocolConfig, RegimeSignal, VaultSigma};
use halcyon_kernel::KernelError;
use halcyon_sol_autocall_quote::midlife::{
    price_midlife_nav_with_matrices, SolAutocallMidlifeError, SolAutocallMidlifeInputs,
    SolAutocallMidlifeMatrixRef, SolAutocallMidlifeStatus,
};

use crate::errors::SolAutocallError;
use crate::pricing::{
    compose_pricing_sigma, protocol_sigma_floor_annualised_s6,
    require_midlife_matrix_commitments_match, require_regime_fresh, require_sigma_fresh,
};
use crate::state::{
    ProductStatus, SolAutocallMidlifeMatrices, SolAutocallTerms, MIDLIFE_MATRIX_LEN,
    MIDLIFE_MATRIX_MAX_STEPS, MIDLIFE_MATRIX_N_STATES, SECONDS_PER_DAY,
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct LendingValuePreview {
    pub nav_s6: i64,
    pub ki_level_usd_s6: i64,
    pub lending_value_s6: i64,
    pub nav_payout_usdc: u64,
    pub lending_value_payout_usdc: u64,
    pub remaining_coupon_pv_s6: i64,
    pub par_recovery_probability_s6: i64,
    pub sigma_pricing_s6: i64,
    pub current_price_s6: i64,
    pub current_observation_index: u8,
    pub due_coupon_count: u8,
    pub future_observation_count: u8,
    pub model_states: u16,
    pub now_ts: i64,
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
    #[account(
        seeds = [seeds::REGIME_SIGNAL, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = regime_signal.product_program_id == crate::ID @ KernelError::ProductProgramMismatch,
    )]
    pub regime_signal: Account<'info, RegimeSignal>,
    #[account(
        constraint = policy_header.product_program_id == crate::ID @ KernelError::ProductProgramMismatch,
        constraint = policy_header.product_terms == product_terms.key() @ SolAutocallError::PolicyStateInvalid,
    )]
    pub policy_header: Account<'info, PolicyHeader>,
    #[account(
        constraint = product_terms.policy_header == policy_header.key() @ SolAutocallError::PolicyStateInvalid,
    )]
    pub product_terms: Account<'info, SolAutocallTerms>,
    #[account(seeds = [seeds::MIDLIFE_MATRICES], bump)]
    pub midlife_matrices: Account<'info, SolAutocallMidlifeMatrices>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_sol: UncheckedAccount<'info>,
    pub clock: Sysvar<'info, Clock>,
}

pub fn handler(ctx: Context<PreviewLendingValue>) -> Result<LendingValuePreview> {
    require!(
        ctx.accounts.policy_header.status == PolicyStatus::Active
            && ctx.accounts.product_terms.status == ProductStatus::Active,
        SolAutocallError::PolicyStateInvalid
    );

    let now = ctx.accounts.clock.unix_timestamp;
    require_sigma_fresh(
        &ctx.accounts.vault_sigma,
        now,
        ctx.accounts.protocol_config.sigma_staleness_cap_secs,
    )?;
    require_regime_fresh(
        &ctx.accounts.regime_signal,
        now,
        ctx.accounts.protocol_config.regime_staleness_cap_secs,
    )?;

    let pyth = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_sol.to_account_info(),
        &halcyon_oracles::feed_ids::SOL_USD,
        &crate::ID,
        &ctx.accounts.clock,
        ctx.accounts.protocol_config.pyth_quote_staleness_cap_secs,
    )?;
    let sigma_pricing_s6 = compose_pricing_sigma(
        &ctx.accounts.vault_sigma,
        &ctx.accounts.regime_signal,
        protocol_sigma_floor_annualised_s6(&ctx.accounts.protocol_config),
    )?;
    require_midlife_matrices_current(
        &ctx.accounts.midlife_matrices,
        sigma_pricing_s6,
        ctx.accounts.vault_sigma.last_update_slot,
        ctx.accounts.regime_signal.last_update_slot,
    )?;
    let mut matrix_refs = Vec::with_capacity(MIDLIFE_MATRIX_MAX_STEPS);
    let matrix_count = ctx.accounts.midlife_matrices.uploaded_step_count as usize;
    for idx in 0..matrix_count {
        let start = idx * MIDLIFE_MATRIX_LEN;
        let end = start + MIDLIFE_MATRIX_LEN;
        matrix_refs.push(SolAutocallMidlifeMatrixRef {
            step_days_s6: ctx.accounts.midlife_matrices.step_days_s6[idx],
            values_s6: &ctx.accounts.midlife_matrices.matrices[start..end],
        });
    }

    let terms = &ctx.accounts.product_terms;
    let nav = price_midlife_nav_with_matrices(
        &SolAutocallMidlifeInputs {
            notional_usdc: ctx.accounts.policy_header.notional,
            entry_price_s6: terms.entry_price_s6,
            current_price_s6: pyth.price_s6,
            autocall_barrier_s6: terms.autocall_barrier_s6,
            coupon_barrier_s6: terms.coupon_barrier_s6,
            ki_barrier_s6: terms.ki_barrier_s6,
            observation_schedule: terms.observation_schedule,
            current_observation_index: terms.current_observation_index,
            no_autocall_first_n_obs: terms.no_autocall_first_n_obs,
            offered_coupon_bps_s6: terms.offered_coupon_bps_s6,
            sigma_annual_s6: sigma_pricing_s6,
            ki_triggered: terms.ki_triggered,
            status: status_for_midlife(terms.status),
            now_ts: now,
            seconds_per_day: SECONDS_PER_DAY,
        },
        &matrix_refs,
    )
    .map_err(map_midlife_error)?;

    Ok(LendingValuePreview {
        nav_s6: nav.nav_s6,
        ki_level_usd_s6: nav.ki_level_s6,
        lending_value_s6: nav.lending_value_s6,
        nav_payout_usdc: nav.nav_payout_usdc,
        lending_value_payout_usdc: nav.lending_value_payout_usdc,
        remaining_coupon_pv_s6: nav.remaining_coupon_pv_s6,
        par_recovery_probability_s6: nav.par_recovery_probability_s6,
        sigma_pricing_s6,
        current_price_s6: pyth.price_s6,
        current_observation_index: terms.current_observation_index,
        due_coupon_count: nav.due_coupon_count,
        future_observation_count: nav.future_observation_count,
        model_states: nav.model_states,
        now_ts: now,
    })
}

fn status_for_midlife(status: ProductStatus) -> SolAutocallMidlifeStatus {
    match status {
        ProductStatus::Active => SolAutocallMidlifeStatus::Active,
        ProductStatus::AutoCalled => SolAutocallMidlifeStatus::AutoCalled,
        ProductStatus::Settled => SolAutocallMidlifeStatus::Settled,
    }
}

fn require_midlife_matrices_current(
    matrices: &SolAutocallMidlifeMatrices,
    sigma_pricing_s6: i64,
    vault_sigma_slot: u64,
    regime_signal_slot: u64,
) -> Result<()> {
    require!(
        matrices.version == SolAutocallMidlifeMatrices::CURRENT_VERSION
            && matrices.sigma_ann_s6 == sigma_pricing_s6
            && matrices.n_states as usize == MIDLIFE_MATRIX_N_STATES
            && matrices.source_vault_sigma_slot == vault_sigma_slot
            && matrices.source_regime_signal_slot == regime_signal_slot
            && matrices.is_complete(),
        SolAutocallError::MidlifeMatricesStale
    );
    require_midlife_matrix_commitments_match(matrices)?;
    Ok(())
}

fn map_midlife_error(_err: SolAutocallMidlifeError) -> Error {
    error!(SolAutocallError::MidlifePricingFailed)
}
