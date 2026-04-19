//! `accept_quote` - issuance path for SOL Autocall.
//!
//! The handler recomputes the live quote at execution slot, enforces the
//! buyer's slippage bounds, reserves capital through the kernel, writes the
//! product terms account, then finalizes the policy in the same transaction.

use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use halcyon_common::{seeds, HalcyonError};
use halcyon_kernel::{
    cpi::accounts::{FinalizePolicy, ReserveAndIssue},
    state::{ProtocolConfig, RegimeSignal, VaultSigma},
    ReserveAndIssueArgs,
};

use crate::pricing::{
    build_observation_schedule, compose_pricing_sigma, derive_barriers_from_entry,
    hash_product_terms, require_protocol_unpaused, require_regime_fresh, require_sigma_fresh,
    solve_quote, ConfidenceGate,
};
use crate::state::{
    ProductStatus, SolAutocallTerms, CURRENT_ENGINE_VERSION, NO_AUTOCALL_FIRST_N_OBS,
};

const SOL_QUOTE_SHARE_BPS: u16 = 7_500;
const SOL_ISSUER_MARGIN_BPS: u16 = 50;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct AcceptQuoteArgs {
    pub policy_id: Pubkey,
    pub notional_usdc: u64,
    pub max_premium: u64,
    pub min_max_liability: u64,
}

#[derive(Accounts)]
#[instruction(args: AcceptQuoteArgs)]
pub struct AcceptQuote<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,

    /// CHECK: kernel-owned `PolicyHeader`, created by `reserve_and_issue`.
    #[account(mut)]
    pub policy_header: UncheckedAccount<'info>,

    #[account(
        init,
        payer = buyer,
        space = 8 + SolAutocallTerms::INIT_SPACE,
        seeds = [seeds::TERMS, args.policy_id.as_ref()],
        bump,
    )]
    pub product_terms: Box<Account<'info, SolAutocallTerms>>,

    /// CHECK: PDA signer for the kernel CPI.
    #[account(seeds = [seeds::PRODUCT_AUTHORITY], bump)]
    pub product_authority: UncheckedAccount<'info>,

    pub usdc_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        constraint = buyer_usdc.mint == usdc_mint.key(),
        constraint = buyer_usdc.owner == buyer.key(),
    )]
    pub buyer_usdc: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [seeds::VAULT_USDC, usdc_mint.key().as_ref()],
        bump,
        constraint = vault_usdc.mint == usdc_mint.key(),
    )]
    pub vault_usdc: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [seeds::TREASURY_USDC, usdc_mint.key().as_ref()],
        bump,
        constraint = treasury_usdc.mint == usdc_mint.key(),
    )]
    pub treasury_usdc: Box<Account<'info, TokenAccount>>,

    /// CHECK: kernel PDA authority for `vault_usdc` / `treasury_usdc`.
    #[account(seeds = [seeds::VAULT_AUTHORITY], bump)]
    pub vault_authority: UncheckedAccount<'info>,

    #[account(mut, seeds = [seeds::PROTOCOL_CONFIG], bump)]
    pub protocol_config: Box<Account<'info, ProtocolConfig>>,

    #[account(
        seeds = [seeds::VAULT_SIGMA, crate::ID.as_ref()],
        bump,
        constraint = vault_sigma.product_program_id == crate::ID,
    )]
    pub vault_sigma: Box<Account<'info, VaultSigma>>,
    #[account(
        seeds = [seeds::REGIME_SIGNAL, crate::ID.as_ref()],
        bump,
        constraint = regime_signal.product_program_id == crate::ID,
    )]
    pub regime_signal: Box<Account<'info, RegimeSignal>>,

    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_sol: UncheckedAccount<'info>,

    #[account(mut, seeds = [seeds::VAULT_STATE], bump)]
    pub vault_state: Box<Account<'info, halcyon_kernel::state::VaultState>>,

    #[account(mut, seeds = [seeds::FEE_LEDGER], bump)]
    pub fee_ledger: Box<Account<'info, halcyon_kernel::state::FeeLedger>>,

    #[account(
        mut,
        seeds = [seeds::PRODUCT_REGISTRY, crate::ID.as_ref()],
        bump,
        constraint = product_registry_entry.product_program_id == crate::ID
            @ halcyon_kernel::KernelError::ProductProgramMismatch,
    )]
    pub product_registry_entry: Box<Account<'info, halcyon_kernel::state::ProductRegistryEntry>>,

    pub clock: Sysvar<'info, Clock>,
    pub kernel_program: Program<'info, halcyon_kernel::program::HalcyonKernel>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<AcceptQuote>, args: AcceptQuoteArgs) -> Result<()> {
    let now = ctx.accounts.clock.unix_timestamp;

    require_protocol_unpaused(&ctx.accounts.protocol_config)?;
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
        ctx.accounts.protocol_config.sigma_floor_annualised_s6,
    )?;

    let quote = solve_quote(
        sigma_pricing_s6,
        args.notional_usdc,
        SOL_QUOTE_SHARE_BPS,
        SOL_ISSUER_MARGIN_BPS,
        now,
        ConfidenceGate::Abort,
    )?;

    require!(
        quote.premium <= args.max_premium,
        HalcyonError::SlippageExceeded
    );
    require!(
        quote.max_liability >= args.min_max_liability,
        HalcyonError::SlippageExceeded
    );

    let (autocall_barrier_s6, coupon_barrier_s6, ki_barrier_s6) =
        derive_barriers_from_entry(pyth.price_s6)?;
    let observation_schedule = build_observation_schedule(now)?;

    let terms = SolAutocallTerms {
        version: SolAutocallTerms::CURRENT_VERSION,
        policy_header: ctx.accounts.policy_header.key(),
        entry_price_s6: pyth.price_s6,
        autocall_barrier_s6,
        coupon_barrier_s6,
        ki_barrier_s6,
        observation_schedule,
        no_autocall_first_n_obs: NO_AUTOCALL_FIRST_N_OBS,
        current_observation_index: 0,
        offered_coupon_bps_s6: quote.offered_coupon_bps_s6,
        quote_share_bps: SOL_QUOTE_SHARE_BPS,
        issuer_margin_bps: SOL_ISSUER_MARGIN_BPS,
        accumulated_coupon_usdc: 0,
        ki_triggered: false,
        status: ProductStatus::Active,
    };
    let terms_hash = hash_product_terms(&terms)?;

    let bump = ctx.bumps.product_authority;
    let signer_seeds: &[&[&[u8]]] = &[&[seeds::PRODUCT_AUTHORITY, &[bump]]];

    halcyon_kernel::cpi::reserve_and_issue(
        CpiContext::new_with_signer(
            ctx.accounts.kernel_program.to_account_info(),
            ReserveAndIssue {
                buyer: ctx.accounts.buyer.to_account_info(),
                product_authority: ctx.accounts.product_authority.to_account_info(),
                usdc_mint: ctx.accounts.usdc_mint.to_account_info(),
                buyer_usdc: ctx.accounts.buyer_usdc.to_account_info(),
                vault_usdc: ctx.accounts.vault_usdc.to_account_info(),
                treasury_usdc: ctx.accounts.treasury_usdc.to_account_info(),
                vault_authority: ctx.accounts.vault_authority.to_account_info(),
                protocol_config: ctx.accounts.protocol_config.to_account_info(),
                vault_state: ctx.accounts.vault_state.to_account_info(),
                fee_ledger: ctx.accounts.fee_ledger.to_account_info(),
                product_registry_entry: ctx.accounts.product_registry_entry.to_account_info(),
                policy_header: ctx.accounts.policy_header.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
            },
            signer_seeds,
        ),
        ReserveAndIssueArgs {
            policy_id: args.policy_id,
            notional: args.notional_usdc,
            premium: quote.premium,
            max_liability: quote.max_liability,
            terms_hash,
            engine_version: CURRENT_ENGINE_VERSION,
            expiry_ts: quote.expiry_ts,
            shard_id: 0,
        },
    )?;

    ctx.accounts.product_terms.set_inner(terms);
    ctx.accounts.product_terms.exit(ctx.program_id)?;

    halcyon_kernel::cpi::finalize_policy(CpiContext::new_with_signer(
        ctx.accounts.kernel_program.to_account_info(),
        FinalizePolicy {
            product_authority: ctx.accounts.product_authority.to_account_info(),
            product_registry_entry: ctx.accounts.product_registry_entry.to_account_info(),
            protocol_config: ctx.accounts.protocol_config.to_account_info(),
            policy_header: ctx.accounts.policy_header.to_account_info(),
            product_terms: ctx.accounts.product_terms.to_account_info(),
        },
        signer_seeds,
    ))?;

    Ok(())
}
