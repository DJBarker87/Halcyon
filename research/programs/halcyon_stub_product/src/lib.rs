//! L1-only stub product used to exercise the kernel's mutual-CPI seam.
//!
//! TODO(L2): delete this crate or relocate to `research/`. No real product
//! should inherit any of its code. It is deliberately trivial so kernel bugs
//! surface as kernel bugs rather than as "something in the pricer".

use anchor_lang::prelude::*;
use anchor_lang::Discriminator;
use anchor_spl::token::{Mint, Token, TokenAccount};
use halcyon_common::seeds;
use halcyon_kernel::{
    cpi::accounts::{FinalizePolicy, ReserveAndIssue},
    instructions::lifecycle::apply_settlement::SettlementReason,
    state::ProductRegistryEntry,
    ReserveAndIssueArgs,
};
use solana_sha256_hasher::hashv;

declare_id!("BHjoWaj82FyupNLgHQTjBCfoNaED4HwQbR2KBNapht1d");

#[program]
pub mod halcyon_stub_product {
    use super::*;

    pub fn accept_quote_stub(ctx: Context<AcceptQuoteStub>, args: StubAcceptArgs) -> Result<()> {
        // --- 1. Predict the terms_hash the kernel will recompute at finalize.
        //
        // ProductTermsStub layout on-disk: [discriminator (8 bytes)] ++
        // [magic u64 little-endian (8 bytes)]. The kernel's
        // `finalize_policy` hashes the FULL account bytes — discriminator
        // included — so the commitment must match exactly.
        let mut blob = [0u8; 16];
        blob[..8].copy_from_slice(ProductTermsStub::DISCRIMINATOR);
        blob[8..].copy_from_slice(&args.magic.to_le_bytes());
        let terms_hash = hashv(&[&blob]).to_bytes();

        // --- 2. CPI kernel::reserve_and_issue ---
        let bump = ctx.bumps.product_authority;
        let seeds_refs: &[&[u8]] = &[seeds::PRODUCT_AUTHORITY, &[bump]];
        let signer_seeds: &[&[&[u8]]] = &[seeds_refs];

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
                notional: args.notional,
                premium: args.premium,
                max_liability: args.max_liability,
                terms_hash,
                engine_version: 1,
                expiry_ts: args.expiry_ts,
                shard_id: 0,
            },
        )?;

        // --- 3. Write ProductTerms locally.
        //
        // Anchor has already init'd the account at handler entry (owner = this
        // program, zero bytes). We only populate its single `magic` field so
        // the on-disk layout matches what we hashed above.
        let terms = &mut ctx.accounts.product_terms;
        terms.magic = args.magic;
        ctx.accounts.product_terms.exit(ctx.program_id)?;

        // --- 4. CPI kernel::finalize_policy — fresh borrow, flips to Active
        //        AFTER rehashing the terms account and comparing.
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

    /// Callback target used when registering the stub in the kernel. Present
    /// for discriminator-registration parity with real products. The stub
    /// writes `ProductTerms` inline in `accept_quote_stub` rather than through
    /// this callback; the handler exists so the registered discriminator
    /// points at a non-empty function that the kernel *could* invoke if the
    /// architecture were ever flipped to the kernel->product callback
    /// direction.
    ///
    /// X3 — writes `magic` to the terms account when called standalone so
    /// regression tests can verify the CPI flow is still wired.
    pub fn init_terms_stub(ctx: Context<InitTermsStub>, magic: u64) -> Result<()> {
        let terms = &mut ctx.accounts.product_terms;
        terms.magic = magic;
        Ok(())
    }

    pub fn settle_stub(ctx: Context<SettleStub>, payout: u64) -> Result<()> {
        let bump = ctx.bumps.product_authority;
        let seeds_refs: &[&[u8]] = &[seeds::PRODUCT_AUTHORITY, &[bump]];
        let signer_seeds: &[&[&[u8]]] = &[seeds_refs];

        halcyon_kernel::cpi::apply_settlement(
            CpiContext::new_with_signer(
                ctx.accounts.kernel_program.to_account_info(),
                halcyon_kernel::cpi::accounts::ApplySettlement {
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
            halcyon_kernel::ApplySettlementArgs {
                payout,
                // Stub defaults to AdminForce so tests can settle at any
                // time without a clock warp. Real products pass Expiry /
                // Autocall / KnockIn per their own schedule.
                reason: SettlementReason::AdminForce,
            },
        )?;
        Ok(())
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct StubAcceptArgs {
    pub policy_id: Pubkey,
    pub notional: u64,
    pub premium: u64,
    pub max_liability: u64,
    pub expiry_ts: i64,
    pub magic: u64,
}

#[account]
#[derive(InitSpace)]
pub struct ProductTermsStub {
    pub magic: u64,
}

#[derive(Accounts)]
#[instruction(args: StubAcceptArgs)]
pub struct AcceptQuoteStub<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,

    /// CHECK: PDA derived from this program's ID; signs for the kernel CPI.
    #[account(seeds = [seeds::PRODUCT_AUTHORITY], bump)]
    pub product_authority: UncheckedAccount<'info>,

    pub usdc_mint: Account<'info, Mint>,

    #[account(mut)]
    pub buyer_usdc: Account<'info, TokenAccount>,

    /// CHECK: kernel-owned, validated in kernel CPI by seed.
    #[account(mut)]
    pub vault_usdc: UncheckedAccount<'info>,

    /// CHECK: kernel-owned, validated in kernel CPI by seed.
    #[account(mut)]
    pub treasury_usdc: UncheckedAccount<'info>,

    /// CHECK: kernel PDA authority.
    pub vault_authority: UncheckedAccount<'info>,

    /// CHECK: kernel PDA.
    #[account(mut)]
    pub protocol_config: UncheckedAccount<'info>,

    /// CHECK: kernel PDA.
    #[account(mut)]
    pub vault_state: UncheckedAccount<'info>,

    /// CHECK: kernel PDA.
    #[account(mut)]
    pub fee_ledger: UncheckedAccount<'info>,

    /// Registry entry for this stub program.
    #[account(mut)]
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,

    /// CHECK: kernel-owned policy header; created by the kernel during CPI.
    #[account(mut)]
    pub policy_header: UncheckedAccount<'info>,

    #[account(
        init,
        payer = buyer,
        space = 8 + ProductTermsStub::INIT_SPACE,
        seeds = [seeds::TERMS, args.policy_id.as_ref()],
        bump,
    )]
    pub product_terms: Account<'info, ProductTermsStub>,

    pub kernel_program: Program<'info, halcyon_kernel::program::HalcyonKernel>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitTermsStub<'info> {
    #[account(mut)]
    pub product_terms: Account<'info, ProductTermsStub>,
}

#[derive(Accounts)]
pub struct SettleStub<'info> {
    /// CHECK: PDA derived from this program's ID.
    #[account(seeds = [seeds::PRODUCT_AUTHORITY], bump)]
    pub product_authority: UncheckedAccount<'info>,

    #[account(mut)]
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,

    /// CHECK: kernel PDA.
    pub protocol_config: UncheckedAccount<'info>,

    /// CHECK: kernel PDA.
    #[account(mut)]
    pub vault_state: UncheckedAccount<'info>,

    /// CHECK: kernel PDA.
    #[account(mut)]
    pub policy_header: UncheckedAccount<'info>,

    pub usdc_mint: Account<'info, Mint>,

    /// CHECK: kernel-owned, validated by kernel.
    #[account(mut)]
    pub vault_usdc: UncheckedAccount<'info>,

    /// CHECK: kernel PDA authority.
    pub vault_authority: UncheckedAccount<'info>,

    #[account(mut)]
    pub buyer_usdc: Account<'info, TokenAccount>,

    pub kernel_program: Program<'info, halcyon_kernel::program::HalcyonKernel>,
    pub token_program: Program<'info, Token>,
}
