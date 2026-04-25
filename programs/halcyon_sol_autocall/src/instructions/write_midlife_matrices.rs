//! `write_midlife_matrices` - keeper-fed exact transition matrices for the
//! SOL autocall midlife Markov pricer.

use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};
use halcyon_kernel::state::{KeeperRegistry, ProtocolConfig, RegimeSignal, VaultSigma};

use crate::{
    errors::SolAutocallError,
    pricing::{
        compose_pricing_sigma, refresh_midlife_matrix_commitments, require_regime_fresh,
        require_sigma_fresh,
    },
    state::{
        SolAutocallMidlifeMatrices, MIDLIFE_MATRIX_LEN, MIDLIFE_MATRIX_MAX_STEPS,
        MIDLIFE_MATRIX_N_STATES,
    },
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct WriteMidlifeMatricesArgs {
    pub begin_upload: bool,
    pub step_index: u16,
    pub step_days_s6: i64,
    pub start: u16,
    pub values: Vec<i64>,
}

#[derive(Accounts)]
pub struct WriteMidlifeMatrices<'info> {
    #[account(mut)]
    pub keeper: Signer<'info>,

    #[account(seeds = [seeds::PROTOCOL_CONFIG], seeds::program = halcyon_kernel::ID, bump)]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(seeds = [seeds::KEEPER_REGISTRY], seeds::program = halcyon_kernel::ID, bump)]
    pub keeper_registry: Account<'info, KeeperRegistry>,

    #[account(
        seeds = [seeds::VAULT_SIGMA, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = vault_sigma.product_program_id == crate::ID,
    )]
    pub vault_sigma: Account<'info, VaultSigma>,

    #[account(
        seeds = [seeds::REGIME_SIGNAL, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = regime_signal.product_program_id == crate::ID,
    )]
    pub regime_signal: Account<'info, RegimeSignal>,

    #[account(
        init_if_needed,
        payer = keeper,
        space = 8 + SolAutocallMidlifeMatrices::INIT_SPACE,
        seeds = [seeds::MIDLIFE_MATRICES],
        bump,
    )]
    pub midlife_matrices: Account<'info, SolAutocallMidlifeMatrices>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<WriteMidlifeMatrices>, args: WriteMidlifeMatricesArgs) -> Result<()> {
    require_keys_eq!(
        ctx.accounts.keeper.key(),
        ctx.accounts.keeper_registry.regime,
        HalcyonError::KeeperAuthorityMismatch
    );

    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
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

    let sigma_ann_s6 = compose_pricing_sigma(
        &ctx.accounts.vault_sigma,
        &ctx.accounts.regime_signal,
        crate::pricing::protocol_sigma_floor_annualised_s6(&ctx.accounts.protocol_config),
    )?;

    let WriteMidlifeMatricesArgs {
        begin_upload,
        step_index,
        step_days_s6,
        start,
        values,
    } = args;
    require!(
        !values.is_empty() && step_days_s6 > 0,
        SolAutocallError::MidlifeMatricesShapeInvalid
    );
    let step_index = step_index as usize;
    let start = start as usize;
    require!(
        step_index < MIDLIFE_MATRIX_MAX_STEPS,
        SolAutocallError::MidlifeMatricesShapeInvalid
    );
    let end = start
        .checked_add(values.len())
        .ok_or(HalcyonError::Overflow)?;
    require!(
        end <= MIDLIFE_MATRIX_LEN,
        SolAutocallError::MidlifeMatricesShapeInvalid
    );
    for value in &values {
        require!(
            (0..=1_000_000).contains(value),
            SolAutocallError::MidlifeMatricesRangeInvalid
        );
    }

    let matrices = &mut ctx.accounts.midlife_matrices;
    if begin_upload {
        matrices.version = SolAutocallMidlifeMatrices::CURRENT_VERSION;
        matrices.sigma_ann_s6 = sigma_ann_s6;
        matrices.n_states = MIDLIFE_MATRIX_N_STATES as u16;
        matrices.cos_terms = halcyon_sol_autocall_quote::midlife::SOL_AUTOCALL_MIDLIFE_COS_TERMS;
        matrices.uploaded_step_count = 0;
        matrices.uploaded_lens = [0u16; MIDLIFE_MATRIX_MAX_STEPS];
        matrices.step_days_s6 = [0i64; MIDLIFE_MATRIX_MAX_STEPS];
        matrices.source_vault_sigma_slot = ctx.accounts.vault_sigma.last_update_slot;
        matrices.source_regime_signal_slot = ctx.accounts.regime_signal.last_update_slot;
        matrices.construction_inputs_sha256 = [0u8; 32];
        matrices.matrix_values_sha256 = [0u8; 32];
        matrices.matrices.clear();
    } else {
        require!(
            matrices.version == SolAutocallMidlifeMatrices::CURRENT_VERSION
                && matrices.sigma_ann_s6 == sigma_ann_s6
                && matrices.n_states as usize == MIDLIFE_MATRIX_N_STATES
                && matrices.cos_terms
                    == halcyon_sol_autocall_quote::midlife::SOL_AUTOCALL_MIDLIFE_COS_TERMS
                && matrices.source_vault_sigma_slot == ctx.accounts.vault_sigma.last_update_slot
                && matrices.source_regime_signal_slot
                    == ctx.accounts.regime_signal.last_update_slot,
            SolAutocallError::MidlifeMatricesUploadStateInvalid
        );
    }

    let uploaded_step_count = matrices.uploaded_step_count as usize;
    if start == 0 && matrices.uploaded_lens[step_index] == 0 {
        require!(
            step_index == uploaded_step_count,
            SolAutocallError::MidlifeMatricesOffsetInvalid
        );
        if step_index > 0 {
            require!(
                matrices.uploaded_lens[step_index - 1] as usize == MIDLIFE_MATRIX_LEN,
                SolAutocallError::MidlifeMatricesOffsetInvalid
            );
        }
        matrices.step_days_s6[step_index] = step_days_s6;
        matrices.uploaded_step_count = (step_index + 1) as u16;
    } else {
        require!(
            step_index < uploaded_step_count && matrices.step_days_s6[step_index] == step_days_s6,
            SolAutocallError::MidlifeMatricesUploadStateInvalid
        );
    }

    let current_len = matrices.uploaded_lens[step_index] as usize;
    require!(
        start == current_len,
        SolAutocallError::MidlifeMatricesOffsetInvalid
    );
    let global_start = step_index
        .checked_mul(MIDLIFE_MATRIX_LEN)
        .and_then(|base| base.checked_add(start))
        .ok_or(HalcyonError::Overflow)?;
    require!(
        matrices.matrices.len() == global_start,
        SolAutocallError::MidlifeMatricesOffsetInvalid
    );

    matrices.matrices.extend(values.into_iter());
    matrices.uploaded_lens[step_index] =
        u16::try_from(end).map_err(|_| error!(HalcyonError::Overflow))?;
    matrices.last_update_slot = clock.slot;
    matrices.last_update_ts = now;
    refresh_midlife_matrix_commitments(matrices)?;

    Ok(())
}
