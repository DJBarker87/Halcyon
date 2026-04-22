//! `write_reduced_operators` - keeper-fed reduced-operator sync for the
//! fixed-product POD-DEIM pricer.

use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};
use halcyon_kernel::state::{KeeperRegistry, ProtocolConfig, RegimeSignal, VaultSigma};
use halcyon_sol_autocall_quote::autocall_v2::MAX_ABS_KEEPER_P_RED_ENTRY_Q20;
use halcyon_sol_autocall_quote::generated::pod_deim_table::POD_DEIM_TABLE_SHA256;

use crate::{
    errors::SolAutocallError,
    pricing::{
        compose_pricing_sigma, require_pod_deim_table_match, require_regime_fresh,
        require_sigma_fresh, KEEPER_DEIM_SIGMA_MAX_S6, KEEPER_DEIM_SIGMA_MIN_S6,
    },
    state::{SolAutocallReducedOperators, REDUCED_OPERATOR_LEN},
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReducedOperatorSide {
    V,
    U,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct WriteReducedOperatorsArgs {
    pub begin_upload: bool,
    pub side: ReducedOperatorSide,
    pub start: u16,
    pub values: Vec<i64>,
}

#[derive(Accounts)]
pub struct WriteReducedOperators<'info> {
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
        space = 8 + SolAutocallReducedOperators::INIT_SPACE,
        seeds = [seeds::REDUCED_OPERATORS],
        bump,
    )]
    pub reduced_operators: Account<'info, SolAutocallReducedOperators>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<WriteReducedOperators>, args: WriteReducedOperatorsArgs) -> Result<()> {
    require_keys_eq!(
        ctx.accounts.keeper.key(),
        ctx.accounts.keeper_registry.regime,
        HalcyonError::KeeperAuthorityMismatch
    );

    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    require_pod_deim_table_match(&ctx.accounts.protocol_config)?;
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
        ctx.accounts.protocol_config.sigma_floor_annualised_s6,
    )?;
    require!(
        (KEEPER_DEIM_SIGMA_MIN_S6..=KEEPER_DEIM_SIGMA_MAX_S6).contains(&sigma_ann_s6),
        SolAutocallError::PricingSigmaOutOfBand
    );

    let WriteReducedOperatorsArgs {
        begin_upload,
        side,
        start,
        values,
    } = args;
    require!(
        !values.is_empty(),
        SolAutocallError::ReducedOperatorsShapeInvalid
    );
    let start = start as usize;
    let end = start
        .checked_add(values.len())
        .ok_or(HalcyonError::Overflow)?;
    require!(
        end <= REDUCED_OPERATOR_LEN,
        SolAutocallError::ReducedOperatorsShapeInvalid
    );
    // Shipping choice: validate the keeper upload once, then keep the runtime
    // matvec branch free of per-quote range checks. A preview-time load check
    // would also be viable, but it would spend compute on every quote instead
    // of every upload.
    for value in &values {
        let abs = value.checked_abs().ok_or(HalcyonError::Overflow)?;
        require!(
            abs <= MAX_ABS_KEEPER_P_RED_ENTRY_Q20,
            SolAutocallError::ReducedOperatorsRangeInvalid
        );
    }

    let reduced = &mut ctx.accounts.reduced_operators;
    if begin_upload {
        reduced.version = SolAutocallReducedOperators::CURRENT_VERSION;
        reduced.sigma_ann_s6 = sigma_ann_s6;
        reduced.source_vault_sigma_slot = ctx.accounts.vault_sigma.last_update_slot;
        reduced.source_regime_signal_slot = ctx.accounts.regime_signal.last_update_slot;
        reduced.pod_deim_table_sha256 = POD_DEIM_TABLE_SHA256;
        reduced.uploaded_v_len = 0;
        reduced.uploaded_u_len = 0;
        reduced.p_red_v.clear();
        reduced.p_red_u.clear();
    } else {
        require!(
            reduced.version == SolAutocallReducedOperators::CURRENT_VERSION
                && reduced.sigma_ann_s6 == sigma_ann_s6
                && reduced.source_vault_sigma_slot == ctx.accounts.vault_sigma.last_update_slot
                && reduced.source_regime_signal_slot
                    == ctx.accounts.regime_signal.last_update_slot
                && reduced.matches_current_tables(),
            SolAutocallError::ReducedOperatorsUploadStateInvalid
        );
    }

    let uploaded_len = match side {
        ReducedOperatorSide::V => {
            require!(
                start == reduced.p_red_v.len(),
                SolAutocallError::ReducedOperatorsOffsetInvalid
            );
            reduced.p_red_v.extend(values.into_iter());
            reduced.p_red_v.len()
        }
        ReducedOperatorSide::U => {
            require!(
                start == reduced.p_red_u.len(),
                SolAutocallError::ReducedOperatorsOffsetInvalid
            );
            reduced.p_red_u.extend(values.into_iter());
            reduced.p_red_u.len()
        }
    };
    let uploaded_len_u16 =
        u16::try_from(uploaded_len).map_err(|_| error!(HalcyonError::Overflow))?;
    match side {
        ReducedOperatorSide::V => reduced.uploaded_v_len = uploaded_len_u16,
        ReducedOperatorSide::U => reduced.uploaded_u_len = uploaded_len_u16,
    }
    reduced.last_update_slot = clock.slot;
    reduced.last_update_ts = now;

    Ok(())
}
