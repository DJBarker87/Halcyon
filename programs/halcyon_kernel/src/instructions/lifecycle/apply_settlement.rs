use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
use halcyon_common::{events::PolicySettled, seeds, HalcyonError};

use crate::{state::*, KernelError};

/// Reason a product may ask the kernel to settle before `expiry_ts`.
///
/// `Expiry` is the default — kernel rejects if `now < expiry_ts`. Non-`Expiry`
/// values document *why* early settlement is legitimate, so a reviewer who
/// sees an early settlement in the event stream can correlate the reason with
/// the product's own records (autocall observation, KI breach, admin force).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum SettlementReason {
    Expiry = 0,
    Autocall = 1,
    KnockIn = 2,
    AdminForce = 3,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct ApplySettlementArgs {
    pub payout: u64,
    pub reason: SettlementReason,
}

#[derive(Accounts)]
pub struct ApplySettlement<'info> {
    pub product_authority: Signer<'info>,

    // See LEARNED.md: seed constraints on kernel-owned PDAs validated inside
    // a product->kernel CPI trigger an Anchor/SBF aliasing bug on 0.32.1.
    // Discriminator-based Account<T> validation is sufficient.
    #[account(mut)]
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,

    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,

    #[account(mut)]
    pub policy_header: Account<'info, PolicyHeader>,

    pub usdc_mint: Account<'info, Mint>,

    #[account(
        mut,
        seeds = [seeds::VAULT_USDC, usdc_mint.key().as_ref()],
        bump,
        token::mint = usdc_mint,
        token::authority = vault_authority,
    )]
    pub vault_usdc: Account<'info, TokenAccount>,

    /// CHECK: PDA authority owning `vault_usdc`.
    #[account(seeds = [seeds::VAULT_AUTHORITY], bump)]
    pub vault_authority: UncheckedAccount<'info>,

    /// K3 — payout destination is pinned to the policy's on-chain owner. A
    /// compromised product authority cannot redirect settlement to an
    /// arbitrary USDC account.
    #[account(
        mut,
        constraint = buyer_usdc.mint == usdc_mint.key(),
        constraint = buyer_usdc.owner == policy_header.owner @ HalcyonError::ProductAuthorityMismatch,
    )]
    pub buyer_usdc: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

pub fn handler(ctx: Context<ApplySettlement>, args: ApplySettlementArgs) -> Result<()> {
    // --- 1. Authentication ---
    require_keys_eq!(
        ctx.accounts.product_authority.key(),
        ctx.accounts.product_registry_entry.expected_authority,
        HalcyonError::ProductAuthorityMismatch
    );
    require_keys_eq!(
        ctx.accounts.policy_header.product_program_id,
        ctx.accounts.product_registry_entry.product_program_id,
        KernelError::ProductProgramMismatch
    );

    // --- 2. Pause gates: global + per-product (K6) ---
    require!(
        !ctx.accounts.protocol_config.settlement_paused_global,
        HalcyonError::SettlementPausedGlobally
    );
    require!(
        !ctx.accounts.product_registry_entry.paused,
        HalcyonError::IssuancePausedPerProduct
    );

    // --- 3. Policy must be Active (settlement is a one-shot) ---
    require!(
        ctx.accounts.policy_header.status == PolicyStatus::Active,
        HalcyonError::PolicyNotActive
    );

    // --- 4. Expiry gate (K14). Only `SettlementReason::Expiry` enforces
    //        `now >= expiry_ts`. Products pass `Autocall`/`KnockIn`/`AdminForce`
    //        when they legitimately settle early; the on-chain event then
    //        records *why*. ---
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    if args.reason == SettlementReason::Expiry {
        require!(
            now >= ctx.accounts.policy_header.expiry_ts,
            HalcyonError::ExpiryNotElapsed
        );
    }

    // --- 5. Clamp payout to reserved max_liability ---
    let max = ctx.accounts.policy_header.max_liability;
    require!(args.payout <= max, KernelError::PayoutExceedsMaxLiability);

    // --- 6. Transfer payout out of the vault ---
    if args.payout > 0 {
        let bump = ctx.bumps.vault_authority;
        let signer_seeds: &[&[&[u8]]] = &[&[seeds::VAULT_AUTHORITY, &[bump]]];
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.vault_usdc.to_account_info(),
                    to: ctx.accounts.buyer_usdc.to_account_info(),
                    authority: ctx.accounts.vault_authority.to_account_info(),
                },
                signer_seeds,
            ),
            args.payout,
        )?;
    }

    // --- 7. Release the unused reservation back to free capital ---
    let released = max.checked_sub(args.payout).ok_or(HalcyonError::Overflow)?;
    let vault = &mut ctx.accounts.vault_state;
    vault.total_reserved_liability = vault
        .total_reserved_liability
        .checked_sub(max)
        .ok_or(HalcyonError::Overflow)?;
    vault.last_update_ts = now;
    vault.last_update_slot = clock.slot;

    // K9 — release the per-product reservation in lockstep so global_risk_cap
    // has a stable meaning. `saturating_sub` would mask bugs; use checked math.
    let registry = &mut ctx.accounts.product_registry_entry;
    registry.total_reserved = registry
        .total_reserved
        .checked_sub(max)
        .ok_or(HalcyonError::Overflow)?;
    registry.last_update_ts = now;

    // --- 8. Mark the header Settled ---
    let header = &mut ctx.accounts.policy_header;
    header.status = PolicyStatus::Settled;
    header.settled_at = now;

    emit!(PolicySettled {
        policy_id: header.key(),
        product_program_id: header.product_program_id,
        owner: header.owner,
        payout: args.payout,
        reservation_released: released,
        settled_at: now,
    });
    Ok(())
}
