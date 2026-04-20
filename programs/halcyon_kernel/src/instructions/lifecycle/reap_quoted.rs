use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};

use crate::state::*;

/// Permissionless cleanup for policies stuck in `Quoted` beyond `expiry_ts`.
///
/// A real product's `accept_quote` always calls `reserve_and_issue` and
/// `finalize_policy` in the same transaction, so a stuck `Quoted` policy
/// indicates the product aborted between the two CPIs (bug, crash, or
/// registered-but-malicious product). Without a reap path, the vault's
/// reservation stays trapped forever: `total_reserved_liability` ticks up
/// and is never released, and the per-product `total_reserved` counter
/// the `global_risk_cap` gate reads from diverges from reality.
///
/// K8 — callable by anyone after `now > quote_expiry_ts`. Releases both the
/// vault-level and product-level reservations. Does NOT refund the premium
/// (the kernel cannot tell who paid, and the product-specific `accept_quote`
/// is a single atomic transaction in every real product; a stuck `Quoted`
/// means the product aborted *after* the kernel already accepted premium).
/// Closing the policy header refunds its rent to the buyer.
#[derive(Accounts)]
pub struct ReapQuoted<'info> {
    /// Anyone can call — rent refund goes to `rent_destination`, which by
    /// convention should be the original buyer (the kernel records it as
    /// `policy_header.owner`).
    #[account(mut)]
    pub rent_destination: SystemAccount<'info>,

    #[account(mut, seeds = [seeds::VAULT_STATE], bump)]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        mut,
        seeds = [seeds::PRODUCT_REGISTRY, policy_header.product_program_id.as_ref()],
        bump,
    )]
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,

    #[account(
        mut,
        constraint = policy_header.product_program_id == product_registry_entry.product_program_id
            @ crate::KernelError::ProductProgramMismatch,
        constraint = rent_destination.key() == policy_header.owner
            @ HalcyonError::NotReapable,
        close = rent_destination,
    )]
    pub policy_header: Account<'info, PolicyHeader>,
}

pub fn handler(ctx: Context<ReapQuoted>) -> Result<()> {
    // Must still be `Quoted` — if finalize_policy already flipped to Active,
    // the normal settle path is required.
    require!(
        ctx.accounts.policy_header.status == PolicyStatus::Quoted,
        HalcyonError::NotReapable
    );

    // Must be past quote_expiry_ts — otherwise a MEV bot races every issuance to
    // close it before the product's own finalize CPI lands.
    let now = Clock::get()?.unix_timestamp;
    require!(
        now > ctx.accounts.policy_header.quote_expiry_ts,
        HalcyonError::NotReapable
    );

    let max = ctx.accounts.policy_header.max_liability;

    // Release vault-level reservation.
    let vault = &mut ctx.accounts.vault_state;
    vault.total_reserved_liability = vault
        .total_reserved_liability
        .checked_sub(max)
        .ok_or(HalcyonError::Overflow)?;
    vault.last_update_ts = now;

    // Release per-product reservation so global_risk_cap stays honest.
    let registry = &mut ctx.accounts.product_registry_entry;
    registry.total_reserved = registry
        .total_reserved
        .checked_sub(max)
        .ok_or(HalcyonError::Overflow)?;
    registry.last_update_ts = now;

    // `close = rent_destination` on the Accounts struct drops lamports and
    // zeros the discriminator for us.
    Ok(())
}
