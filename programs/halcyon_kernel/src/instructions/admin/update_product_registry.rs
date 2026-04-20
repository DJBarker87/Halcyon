use anchor_lang::prelude::*;
use halcyon_common::{events::ConfigUpdated, seeds, HalcyonError};

use crate::{state::*, KernelError};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct UpdateProductRegistryArgs {
    pub product_program_id: Pubkey,
    pub active: Option<bool>,
    pub paused: Option<bool>,
    pub per_policy_risk_cap: Option<u64>,
    pub global_risk_cap: Option<u64>,
    pub engine_version: Option<u16>,
    pub init_terms_discriminator: Option<[u8; 8]>,
}

#[derive(Accounts)]
#[instruction(args: UpdateProductRegistryArgs)]
pub struct UpdateProductRegistry<'info> {
    pub admin: Signer<'info>,

    #[account(
        seeds = [seeds::PROTOCOL_CONFIG],
        bump,
        has_one = admin @ HalcyonError::AdminMismatch,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(
        mut,
        seeds = [seeds::PRODUCT_REGISTRY, args.product_program_id.as_ref()],
        bump,
    )]
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,
}

pub fn handler(ctx: Context<UpdateProductRegistry>, args: UpdateProductRegistryArgs) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let entry = &mut ctx.accounts.product_registry_entry;
    if let Some(x) = args.active {
        entry.active = x;
    }
    if let Some(x) = args.paused {
        entry.paused = x;
    }
    if let Some(x) = args.per_policy_risk_cap {
        entry.per_policy_risk_cap = x;
    }
    if let Some(x) = args.global_risk_cap {
        // M-4 — the admin must not lower `global_risk_cap` below live
        // reservations. Doing so would leave the accounting invariant
        // `total_reserved ≤ global_risk_cap` silently false until enough
        // policies settle, which rots downstream risk telemetry.
        require!(x >= entry.total_reserved, KernelError::BadConfig);
        entry.global_risk_cap = x;
    }
    if let Some(x) = args.engine_version {
        entry.engine_version = x;
    }
    if let Some(d) = args.init_terms_discriminator {
        // M-4 — discriminator rotation while reservations are live
        // invalidates every in-flight `finalize_policy` on this product
        // (the kernel rehashes against the stored discriminator). When
        // `total_reserved > 0` we require the same call to also set
        // `paused = true` so no new issuance lands and any stuck Quoted
        // reservation can be reaped via `reap_quoted` without confusing
        // buyers. No-op rotation (same bytes) is always allowed.
        let rotating = d != entry.init_terms_discriminator;
        if rotating && entry.total_reserved > 0 {
            // Reading `entry.paused` directly reflects any mutation made
            // earlier in this same handler (see the `args.paused` block
            // above), so pairing `paused: true` with the rotation
            // satisfies this gate.
            require!(entry.paused, KernelError::BadConfig);
        }
        entry.init_terms_discriminator = d;
    }
    entry.last_update_ts = now;
    emit!(ConfigUpdated {
        admin: ctx.accounts.protocol_config.admin,
        field_tag: 3,
        updated_at: now,
    });
    Ok(())
}
