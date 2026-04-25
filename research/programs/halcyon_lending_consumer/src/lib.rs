//! Reference lending-consumer program for the thesis-pivot flow.
//!
//! The important property is not a full lending market. It is that a third
//! party can own a live policy through a PDA escrow and liquidate the seized
//! collateral by CPI-ing into the issuer's deterministic buyback instruction.

use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use halcyon_common::HalcyonError;
use halcyon_flagship_autocall::{
    cpi::accounts::Buyback as FlagshipBuyback, program::HalcyonFlagshipAutocall,
    state::FlagshipAutocallTerms,
};
use halcyon_kernel::state::{
    PolicyHeader, ProductRegistryEntry, ProtocolConfig, Regression, VaultSigma, VaultState,
};

declare_id!("BSZABrfDG1vN3q7sejfebPFbqfqwVRu8gcjSukWEiXqF");

pub const ESCROW_SEED: &[u8] = b"policy_escrow";

#[program]
pub mod halcyon_lending_consumer {
    use super::*;

    pub fn price_note(ctx: Context<PriceNote>, args: PriceNoteArgs) -> Result<()> {
        emit!(NotePriced {
            payer: ctx.accounts.payer.key(),
            receipt_mint: ctx.accounts.receipt_mint.key(),
            fair_value_usdc: args.fair_value_usdc,
            lending_value_usdc: args.lending_value_usdc,
            max_borrow_usdc: args.max_borrow_usdc,
            source_slot: args.source_slot,
            slot: ctx.accounts.clock.slot,
        });

        Ok(())
    }

    pub fn issue_loan(ctx: Context<IssueLoan>, args: IssueLoanArgs) -> Result<()> {
        emit!(LoanIssued {
            lender: ctx.accounts.lender.key(),
            borrower: ctx.accounts.borrower.key(),
            receipt_mint: ctx.accounts.receipt_mint.key(),
            loan_id: args.loan_id,
            principal_usdc: args.principal_usdc,
            lending_value_usdc: args.lending_value_usdc,
            debt_usdc: args.debt_usdc,
            slot: ctx.accounts.clock.slot,
        });

        Ok(())
    }

    pub fn liquidate_flagship_note(ctx: Context<LiquidateFlagshipNote>) -> Result<()> {
        require_keys_eq!(
            ctx.accounts.policy_header.owner,
            ctx.accounts.escrow_authority.key(),
            HalcyonError::ProductAuthorityMismatch
        );
        require_keys_eq!(
            ctx.accounts.policy_header.product_program_id,
            halcyon_flagship_autocall::ID,
            HalcyonError::ProductAuthorityMismatch
        );

        let policy_key = ctx.accounts.policy_header.key();
        let bump = ctx.bumps.escrow_authority;
        let escrow_seeds: &[&[u8]] = &[ESCROW_SEED, policy_key.as_ref(), &[bump]];
        let signer_seeds: &[&[&[u8]]] = &[escrow_seeds];

        halcyon_flagship_autocall::cpi::buyback(CpiContext::new_with_signer(
            ctx.accounts.flagship_program.to_account_info(),
            FlagshipBuyback {
                policy_owner: ctx.accounts.escrow_authority.to_account_info(),
                policy_header: ctx.accounts.policy_header.to_account_info(),
                product_terms: ctx.accounts.product_terms.to_account_info(),
                product_registry_entry: ctx.accounts.product_registry_entry.to_account_info(),
                protocol_config: ctx.accounts.protocol_config.to_account_info(),
                vault_sigma: ctx.accounts.vault_sigma.to_account_info(),
                regression: ctx.accounts.regression.to_account_info(),
                pyth_spy: ctx.accounts.pyth_spy.to_account_info(),
                pyth_qqq: ctx.accounts.pyth_qqq.to_account_info(),
                pyth_iwm: ctx.accounts.pyth_iwm.to_account_info(),
                usdc_mint: ctx.accounts.usdc_mint.to_account_info(),
                vault_usdc: ctx.accounts.vault_usdc.to_account_info(),
                vault_authority: ctx.accounts.vault_authority.to_account_info(),
                owner_usdc: ctx.accounts.escrow_usdc.to_account_info(),
                product_authority: ctx.accounts.product_authority.to_account_info(),
                vault_state: ctx.accounts.vault_state.to_account_info(),
                clock: ctx.accounts.clock.to_account_info(),
                kernel_program: ctx.accounts.kernel_program.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            },
            signer_seeds,
        ))
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct PriceNoteArgs {
    pub fair_value_usdc: u64,
    pub lending_value_usdc: u64,
    pub max_borrow_usdc: u64,
    pub source_slot: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct IssueLoanArgs {
    pub loan_id: u64,
    pub principal_usdc: u64,
    pub lending_value_usdc: u64,
    pub debt_usdc: u64,
}

#[event]
pub struct NotePriced {
    pub payer: Pubkey,
    pub receipt_mint: Pubkey,
    pub fair_value_usdc: u64,
    pub lending_value_usdc: u64,
    pub max_borrow_usdc: u64,
    pub source_slot: u64,
    pub slot: u64,
}

#[event]
pub struct LoanIssued {
    pub lender: Pubkey,
    pub borrower: Pubkey,
    pub receipt_mint: Pubkey,
    pub loan_id: u64,
    pub principal_usdc: u64,
    pub lending_value_usdc: u64,
    pub debt_usdc: u64,
    pub slot: u64,
}

#[derive(Accounts)]
pub struct PriceNote<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: Demo receipt mint only. The preceding Flagship instruction is
    /// the source of on-chain pricing in the same transaction.
    pub receipt_mint: UncheckedAccount<'info>,
    pub clock: Sysvar<'info, Clock>,
}

#[derive(Accounts)]
pub struct IssueLoan<'info> {
    #[account(mut)]
    pub lender: Signer<'info>,
    /// CHECK: Demo borrower marker for the reference lending flow.
    pub borrower: UncheckedAccount<'info>,
    /// CHECK: Demo receipt mint being pledged as collateral.
    pub receipt_mint: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    pub clock: Sysvar<'info, Clock>,
}

#[derive(Accounts)]
pub struct LiquidateFlagshipNote<'info> {
    #[account(
        mut,
        constraint = policy_header.product_terms == product_terms.key()
            @ HalcyonError::ProductAuthorityMismatch,
    )]
    pub policy_header: Box<Account<'info, PolicyHeader>>,

    /// CHECK: PDA owned by this reference consumer. The holder transfers the
    /// policy owner field to this address before the lending protocol can
    /// seize and liquidate it.
    #[account(seeds = [ESCROW_SEED, policy_header.key().as_ref()], bump)]
    pub escrow_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        constraint = product_terms.policy_header == policy_header.key()
            @ HalcyonError::ProductAuthorityMismatch,
    )]
    pub product_terms: Box<Account<'info, FlagshipAutocallTerms>>,

    #[account(mut)]
    pub product_registry_entry: Box<Account<'info, ProductRegistryEntry>>,
    pub protocol_config: Box<Account<'info, ProtocolConfig>>,
    pub vault_sigma: Box<Account<'info, VaultSigma>>,
    pub regression: Box<Account<'info, Regression>>,

    /// CHECK: validated by `halcyon_oracles` inside the flagship CPI.
    pub pyth_spy: UncheckedAccount<'info>,
    /// CHECK: validated by `halcyon_oracles` inside the flagship CPI.
    pub pyth_qqq: UncheckedAccount<'info>,
    /// CHECK: validated by `halcyon_oracles` inside the flagship CPI.
    pub pyth_iwm: UncheckedAccount<'info>,

    pub usdc_mint: Box<Account<'info, Mint>>,
    #[account(mut)]
    pub vault_usdc: Box<Account<'info, TokenAccount>>,
    /// CHECK: validated by the flagship CPI.
    pub vault_authority: UncheckedAccount<'info>,
    #[account(mut)]
    pub escrow_usdc: Box<Account<'info, TokenAccount>>,
    /// CHECK: validated by the flagship CPI.
    pub product_authority: UncheckedAccount<'info>,
    #[account(mut)]
    pub vault_state: Box<Account<'info, VaultState>>,

    pub clock: Sysvar<'info, Clock>,
    pub flagship_program: Program<'info, HalcyonFlagshipAutocall>,
    pub kernel_program: Program<'info, halcyon_kernel::program::HalcyonKernel>,
    pub token_program: Program<'info, Token>,
}
