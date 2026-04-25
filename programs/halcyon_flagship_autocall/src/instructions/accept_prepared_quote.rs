use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::get_associated_token_address,
    token::{Mint, Token, TokenAccount},
};
use halcyon_common::{seeds, HalcyonError};
use halcyon_kernel::{
    cpi::accounts::{FinalizePolicy, ReserveAndIssue},
    state::{AutocallSchedule, CouponSchedule, ProtocolConfig},
    ReserveAndIssueArgs,
};

use crate::pricing::{
    hash_product_terms, monthly_coupon_schedule_from_account,
    quarterly_autocall_schedule_from_account, require_autocall_schedule_fresh,
    require_coupon_schedule_fresh, require_coupon_schedule_matches_autocall,
    require_protocol_unpaused, require_quote_acceptance_bounds, require_sigma_fresh, QuoteOutputs,
};
use crate::state::{
    FlagshipAutocallTerms, FlagshipQuoteReceipt, ProductStatus, AUTOCALL_BARRIER_BPS,
    COUPON_BARRIER_BPS, CURRENT_ENGINE_VERSION, KI_BARRIER_BPS,
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct AcceptPreparedQuoteArgs {
    pub max_premium: u64,
    pub min_max_liability: u64,
    pub min_offered_coupon_bps_s6: i64,
    pub max_quote_slot_delta: u64,
    pub max_entry_price_deviation_bps: u16,
    pub max_expiry_delta_secs: i64,
}

#[derive(Accounts)]
pub struct AcceptPreparedQuote<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,

    #[account(
        mut,
        close = buyer,
        constraint = quote_receipt.version == FlagshipQuoteReceipt::CURRENT_VERSION,
        constraint = quote_receipt.buyer == buyer.key(),
    )]
    pub quote_receipt: Box<Account<'info, FlagshipQuoteReceipt>>,

    /// CHECK: kernel-owned `PolicyHeader`, created by `reserve_and_issue`.
    #[account(mut)]
    pub policy_header: UncheckedAccount<'info>,

    #[account(
        init,
        payer = buyer,
        space = 8 + FlagshipAutocallTerms::INIT_SPACE,
        seeds = [seeds::TERMS, quote_receipt.policy_id.as_ref()],
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
    pub vault_sigma: Box<Account<'info, halcyon_kernel::state::VaultSigma>>,

    #[account(
        seeds = [seeds::AUTOCALL_SCHEDULE, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = autocall_schedule.product_program_id == crate::ID,
    )]
    pub autocall_schedule: Box<Account<'info, AutocallSchedule>>,

    #[account(
        seeds = [seeds::COUPON_SCHEDULE, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = coupon_schedule.product_program_id == crate::ID,
    )]
    pub coupon_schedule: Box<Account<'info, CouponSchedule>>,

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

pub fn handler(ctx: Context<AcceptPreparedQuote>, args: AcceptPreparedQuoteArgs) -> Result<()> {
    let now = ctx.accounts.clock.unix_timestamp;
    let quote_receipt = &ctx.accounts.quote_receipt;

    require_protocol_unpaused(&ctx.accounts.protocol_config)?;
    require_sigma_fresh(
        &ctx.accounts.vault_sigma,
        now,
        ctx.accounts.protocol_config.sigma_staleness_cap_secs,
    )?;
    require_autocall_schedule_fresh(&ctx.accounts.autocall_schedule, now)?;
    require_coupon_schedule_fresh(&ctx.accounts.coupon_schedule, now)?;
    require_coupon_schedule_matches_autocall(
        &ctx.accounts.coupon_schedule,
        &ctx.accounts.autocall_schedule,
    )?;

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

    let quote = QuoteOutputs {
        premium: quote_receipt.premium,
        max_liability: quote_receipt.max_liability,
        fair_coupon_bps_s6: quote_receipt.fair_coupon_bps_s6,
        offered_coupon_bps_s6: quote_receipt.offered_coupon_bps_s6,
        sigma_pricing_s6: quote_receipt.sigma_pricing_s6,
        quote_slot: quote_receipt.quote_slot,
        expiry_ts: quote_receipt.expiry_ts,
        engine_version: CURRENT_ENGINE_VERSION,
    };

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
        quote_receipt.quote_slot,
        args.max_quote_slot_delta,
        pyth_spy.price_s6,
        quote_receipt.entry_spy_price_s6,
        pyth_qqq.price_s6,
        quote_receipt.entry_qqq_price_s6,
        pyth_iwm.price_s6,
        quote_receipt.entry_iwm_price_s6,
        args.max_entry_price_deviation_bps,
        quote_receipt.expiry_ts,
        args.max_expiry_delta_secs,
    )?;

    let terms = FlagshipAutocallTerms {
        version: FlagshipAutocallTerms::CURRENT_VERSION,
        policy_header: ctx.accounts.policy_header.key(),
        entry_spy_price_s6: quote_receipt.entry_spy_price_s6,
        entry_qqq_price_s6: quote_receipt.entry_qqq_price_s6,
        entry_iwm_price_s6: quote_receipt.entry_iwm_price_s6,
        monthly_coupon_schedule: monthly_coupon_schedule_from_account(
            &ctx.accounts.coupon_schedule,
        )?,
        quarterly_autocall_schedule: quarterly_autocall_schedule_from_account(
            &ctx.accounts.autocall_schedule,
        )?,
        next_coupon_index: 0,
        next_autocall_index: 0,
        offered_coupon_bps_s6: quote.offered_coupon_bps_s6,
        coupon_barrier_bps: COUPON_BARRIER_BPS,
        autocall_barrier_bps: AUTOCALL_BARRIER_BPS,
        ki_barrier_bps: KI_BARRIER_BPS,
        missed_coupon_observations: 0,
        ki_latched: false,
        coupons_paid_usdc: 0,
        beta_spy_s12: quote_receipt.beta_spy_s12,
        beta_qqq_s12: quote_receipt.beta_qqq_s12,
        alpha_s12: quote_receipt.alpha_s12,
        regression_r_squared_s6: quote_receipt.regression_r_squared_s6,
        regression_residual_vol_s6: quote_receipt.regression_residual_vol_s6,
        k12_correction_sha256: crate::pricing::K12_CORRECTION_SHA256,
        daily_ki_correction_sha256: crate::pricing::DAILY_KI_CORRECTION_SHA256,
        settled_payout_usdc: 0,
        settled_at: 0,
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
            policy_id: quote_receipt.policy_id,
            notional: quote_receipt.notional_usdc,
            premium: quote.premium,
            vault_deposit_amount: quote_receipt.notional_usdc,
            max_liability: quote.max_liability,
            terms_hash,
            engine_version: quote.engine_version,
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
