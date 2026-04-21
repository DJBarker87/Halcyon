use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::get_associated_token_address,
    token::{Mint, Token, TokenAccount},
};
use halcyon_common::{seeds, HalcyonError};
use halcyon_il_quote::{CAP_S12, DEDUCTIBLE_S12, POOL_WEIGHT_S12};
use halcyon_kernel::{
    cpi::accounts::ApplySettlement,
    state::{PolicyHeader, PolicyStatus, ProductRegistryEntry, ProtocolConfig, VaultState},
    ApplySettlementArgs, KernelError, SettlementReason,
};

use crate::errors::IlProtectionError;
use crate::state::{IlProtectionTerms, ProductStatus};

#[derive(Accounts)]
pub struct Settle<'info> {
    pub caller: Signer<'info>,

    #[account(mut)]
    pub policy_header: Box<Account<'info, PolicyHeader>>,

    #[account(
        mut,
        constraint = product_terms.policy_header == policy_header.key() @ IlProtectionError::PolicyStateInvalid,
    )]
    pub product_terms: Box<Account<'info, IlProtectionTerms>>,

    #[account(
        mut,
        seeds = [seeds::PRODUCT_REGISTRY, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = product_registry_entry.product_program_id == crate::ID
            @ KernelError::ProductProgramMismatch,
    )]
    pub product_registry_entry: Box<Account<'info, ProductRegistryEntry>>,

    #[account(seeds = [seeds::PROTOCOL_CONFIG], seeds::program = halcyon_kernel::ID, bump)]
    pub protocol_config: Box<Account<'info, ProtocolConfig>>,

    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_sol: UncheckedAccount<'info>,
    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_usdc: UncheckedAccount<'info>,

    pub usdc_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        seeds = [seeds::VAULT_USDC, usdc_mint.key().as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = vault_usdc.mint == usdc_mint.key(),
    )]
    pub vault_usdc: Box<Account<'info, TokenAccount>>,

    /// CHECK: kernel PDA authority for `vault_usdc`.
    #[account(seeds = [seeds::VAULT_AUTHORITY], seeds::program = halcyon_kernel::ID, bump)]
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

    #[account(mut, seeds = [seeds::VAULT_STATE], seeds::program = halcyon_kernel::ID, bump)]
    pub vault_state: Box<Account<'info, VaultState>>,

    pub clock: Sysvar<'info, Clock>,
    pub kernel_program: Program<'info, halcyon_kernel::program::HalcyonKernel>,
    pub token_program: Program<'info, Token>,
}

pub fn handler(ctx: Context<Settle>) -> Result<()> {
    if ctx.accounts.policy_header.status != PolicyStatus::Active
        || ctx.accounts.product_terms.status != ProductStatus::Active
    {
        return Ok(());
    }

    // L3-M2 — short-circuit when settlement is globally paused so we do
    // not pay compute for the full Pyth + IL-math pipeline before the
    // kernel's apply_settlement CPI rejects.
    require!(
        !ctx.accounts.protocol_config.settlement_paused_global,
        HalcyonError::SettlementPausedGlobally
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

    let now = ctx.accounts.clock.unix_timestamp;
    require!(
        now >= ctx.accounts.product_terms.expiry_ts,
        IlProtectionError::PolicyNotExpired
    );

    let pyth_sol = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_sol.to_account_info(),
        &halcyon_oracles::feed_ids::SOL_USD,
        &crate::ID,
        &ctx.accounts.clock,
        ctx.accounts.protocol_config.pyth_settle_staleness_cap_secs,
    )?;
    let pyth_usdc = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_usdc.to_account_info(),
        &halcyon_oracles::feed_ids::USDC_USD,
        &crate::ID,
        &ctx.accounts.clock,
        ctx.accounts.protocol_config.pyth_settle_staleness_cap_secs,
    )?;

    let entry_sol_price_s12 = price_s6_to_s12(ctx.accounts.product_terms.entry_sol_price_s6)?;
    let entry_usdc_price_s12 = price_s6_to_s12(ctx.accounts.product_terms.entry_usdc_price_s6)?;
    let exit_sol_price_s12 = price_s6_to_s12(pyth_sol.price_s6)?;
    let exit_usdc_price_s12 = price_s6_to_s12(pyth_usdc.price_s6)?;
    let (terminal_il_s12, payout) =
        halcyon_il_quote::insurance::settlement::compute_settlement_from_prices(
            POOL_WEIGHT_S12,
            exit_sol_price_s12,
            exit_usdc_price_s12,
            entry_sol_price_s12,
            entry_usdc_price_s12,
            ctx.accounts.product_terms.insured_notional_usdc,
            DEDUCTIBLE_S12,
            CAP_S12,
        )
        .map_err(|_| error!(IlProtectionError::SettlementComputationFailed))?;

    {
        let terms = &mut ctx.accounts.product_terms;
        terms.settled_terminal_il_s12 = terminal_il_s12;
        terms.settled_payout_usdc = payout;
        terms.settled_at = now;
        terms.status = ProductStatus::Settled;
    }

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
        ApplySettlementArgs {
            payout,
            reason: SettlementReason::Expiry,
        },
    )
}

fn price_s6_to_s12(price_s6: i64) -> Result<u128> {
    require!(price_s6 > 0, IlProtectionError::InvalidEntryPrice);
    (price_s6 as u128)
        .checked_mul(1_000_000u128)
        .ok_or_else(|| error!(HalcyonError::Overflow))
}
