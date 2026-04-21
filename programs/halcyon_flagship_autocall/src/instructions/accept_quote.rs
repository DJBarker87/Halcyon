use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::get_associated_token_address,
    token::{Mint, Token, TokenAccount},
};
use halcyon_common::{seeds, HalcyonError};
use halcyon_kernel::{
    cpi::accounts::{FinalizePolicy, ReserveAndIssue},
    state::{ProtocolConfig, Regression, VaultSigma},
    ReserveAndIssueArgs,
};

use crate::pricing::{
    build_monthly_coupon_schedule, build_quarterly_autocall_schedule, compose_pricing_sigma,
    hash_product_terms, require_correction_tables_match, require_protocol_unpaused,
    require_quote_acceptance_bounds, require_regression_fresh, require_sigma_fresh, solve_quote,
};
use crate::state::{
    FlagshipAutocallTerms, ProductStatus, AUTOCALL_BARRIER_BPS, COUPON_BARRIER_BPS,
    CURRENT_ENGINE_VERSION, KI_BARRIER_BPS,
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct AcceptQuoteArgs {
    pub policy_id: Pubkey,
    pub notional_usdc: u64,
    pub max_premium: u64,
    pub min_max_liability: u64,
    pub min_offered_coupon_bps_s6: i64,
    pub preview_quote_slot: u64,
    pub max_quote_slot_delta: u64,
    pub preview_entry_spy_price_s6: i64,
    pub preview_entry_qqq_price_s6: i64,
    pub preview_entry_iwm_price_s6: i64,
    pub max_entry_price_deviation_bps: u16,
    pub preview_expiry_ts: i64,
    pub max_expiry_delta_secs: i64,
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
        space = 8 + FlagshipAutocallTerms::INIT_SPACE,
        seeds = [seeds::TERMS, args.policy_id.as_ref()],
        bump,
    )]
    pub product_terms: Box<Account<'info, FlagshipAutocallTerms>>,

    /// CHECK: PDA signer for the kernel CPI.
    #[account(seeds = [seeds::PRODUCT_AUTHORITY], bump)]
    pub product_authority: UncheckedAccount<'info>,

    pub usdc_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        constraint = buyer_usdc.mint == usdc_mint.key(),
        constraint = buyer_usdc.owner == buyer.key(),
        constraint = buyer_usdc.key()
            == get_associated_token_address(&buyer.key(), &usdc_mint.key())
            @ HalcyonError::ProductAuthorityMismatch,
    )]
    pub buyer_usdc: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [seeds::VAULT_USDC, usdc_mint.key().as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = vault_usdc.mint == usdc_mint.key(),
    )]
    pub vault_usdc: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [seeds::TREASURY_USDC, usdc_mint.key().as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = treasury_usdc.mint == usdc_mint.key(),
    )]
    pub treasury_usdc: Box<Account<'info, TokenAccount>>,

    /// CHECK: kernel PDA authority for `vault_usdc` / `treasury_usdc`.
    #[account(seeds = [seeds::VAULT_AUTHORITY], seeds::program = halcyon_kernel::ID, bump)]
    pub vault_authority: UncheckedAccount<'info>,

    #[account(mut, seeds = [seeds::PROTOCOL_CONFIG], seeds::program = halcyon_kernel::ID, bump)]
    pub protocol_config: Box<Account<'info, ProtocolConfig>>,

    #[account(
        seeds = [seeds::VAULT_SIGMA, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = vault_sigma.product_program_id == crate::ID,
    )]
    pub vault_sigma: Box<Account<'info, VaultSigma>>,

    #[account(seeds = [seeds::REGRESSION], seeds::program = halcyon_kernel::ID, bump)]
    pub regression: Box<Account<'info, Regression>>,

    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_spy: UncheckedAccount<'info>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_qqq: UncheckedAccount<'info>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_iwm: UncheckedAccount<'info>,

    #[account(mut, seeds = [seeds::VAULT_STATE], seeds::program = halcyon_kernel::ID, bump)]
    pub vault_state: Box<Account<'info, halcyon_kernel::state::VaultState>>,

    #[account(mut, seeds = [seeds::FEE_LEDGER], seeds::program = halcyon_kernel::ID, bump)]
    pub fee_ledger: Box<Account<'info, halcyon_kernel::state::FeeLedger>>,

    #[account(
        mut,
        seeds = [seeds::PRODUCT_REGISTRY, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = product_registry_entry.product_program_id == crate::ID
            @ halcyon_kernel::KernelError::ProductProgramMismatch,
        constraint = product_registry_entry.active
            @ halcyon_common::HalcyonError::ProductNotRegistered,
        constraint = !product_registry_entry.paused
            @ halcyon_common::HalcyonError::IssuancePausedPerProduct,
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
    require_regression_fresh(
        &ctx.accounts.regression,
        now,
        ctx.accounts.protocol_config.regression_staleness_cap_secs,
    )?;
    require_correction_tables_match(&ctx.accounts.protocol_config)?;

    let pyth_spy = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_spy.to_account_info(),
        &halcyon_oracles::feed_ids::SPY_USD,
        &crate::ID,
        &ctx.accounts.clock,
        ctx.accounts.protocol_config.pyth_quote_staleness_cap_secs,
    )?;
    let pyth_qqq = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_qqq.to_account_info(),
        &halcyon_oracles::feed_ids::QQQ_USD,
        &crate::ID,
        &ctx.accounts.clock,
        ctx.accounts.protocol_config.pyth_quote_staleness_cap_secs,
    )?;
    let pyth_iwm = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_iwm.to_account_info(),
        &halcyon_oracles::feed_ids::IWM_USD,
        &crate::ID,
        &ctx.accounts.clock,
        ctx.accounts.protocol_config.pyth_quote_staleness_cap_secs,
    )?;

    let sigma_pricing_s6 = compose_pricing_sigma(
        &ctx.accounts.vault_sigma,
        ctx.accounts.protocol_config.sigma_floor_annualised_s6,
    )?;
    let quote = solve_quote(sigma_pricing_s6, args.notional_usdc, now)?;

    require!(
        quote.premium <= args.max_premium,
        HalcyonError::SlippageExceeded
    );
    require!(
        quote.max_liability >= args.min_max_liability,
        HalcyonError::SlippageExceeded
    );
    require_quote_acceptance_bounds(
        &quote,
        args.min_offered_coupon_bps_s6,
        args.preview_quote_slot,
        args.max_quote_slot_delta,
        pyth_spy.price_s6,
        args.preview_entry_spy_price_s6,
        pyth_qqq.price_s6,
        args.preview_entry_qqq_price_s6,
        pyth_iwm.price_s6,
        args.preview_entry_iwm_price_s6,
        args.max_entry_price_deviation_bps,
        args.preview_expiry_ts,
        args.max_expiry_delta_secs,
    )?;

    let terms = FlagshipAutocallTerms {
        version: FlagshipAutocallTerms::CURRENT_VERSION,
        policy_header: ctx.accounts.policy_header.key(),
        entry_spy_price_s6: pyth_spy.price_s6,
        entry_qqq_price_s6: pyth_qqq.price_s6,
        entry_iwm_price_s6: pyth_iwm.price_s6,
        monthly_coupon_schedule: build_monthly_coupon_schedule(now)?,
        quarterly_autocall_schedule: build_quarterly_autocall_schedule(now)?,
        next_coupon_index: 0,
        next_autocall_index: 0,
        offered_coupon_bps_s6: quote.offered_coupon_bps_s6,
        coupon_barrier_bps: COUPON_BARRIER_BPS,
        autocall_barrier_bps: AUTOCALL_BARRIER_BPS,
        ki_barrier_bps: KI_BARRIER_BPS,
        missed_coupon_observations: 0,
        ki_latched: false,
        coupons_paid_usdc: 0,
        beta_spy_s12: ctx.accounts.regression.beta_spy_s12,
        beta_qqq_s12: ctx.accounts.regression.beta_qqq_s12,
        alpha_s12: ctx.accounts.regression.alpha_s12,
        regression_r_squared_s6: ctx.accounts.regression.r_squared_s6,
        regression_residual_vol_s6: ctx.accounts.regression.residual_vol_s6,
        k12_correction_sha256: crate::pricing::K12_CORRECTION_SHA256,
        daily_ki_correction_sha256: crate::pricing::DAILY_KI_CORRECTION_SHA256,
        settled_payout_usdc: 0,
        settled_at: 0,
        status: ProductStatus::Active,
    };
    let terms_hash = hash_product_terms(&terms)?;
    let vault_deposit_amount = args.notional_usdc;

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
            vault_deposit_amount,
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
