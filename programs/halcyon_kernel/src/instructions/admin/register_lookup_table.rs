use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};

use crate::{state::*, KernelError};

#[derive(Accounts)]
#[instruction(lookup_table: Pubkey)]
pub struct RegisterLookupTable<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        seeds = [seeds::PROTOCOL_CONFIG],
        bump,
        has_one = admin @ HalcyonError::AdminMismatch,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    /// Per-product (or protocol-wide using Pubkey::default()) registry; created
    /// on first registration.
    #[account(
        init_if_needed,
        payer = admin,
        space = 8 + LookupTableRegistry::INIT_SPACE,
        seeds = [seeds::ALT_REGISTRY, product_program_id.key().as_ref()],
        bump,
    )]
    pub lookup_table_registry: Account<'info, LookupTableRegistry>,

    /// CHECK: pubkey identifying the per-product registry shard. Pass
    /// `Pubkey::default()` for a protocol-wide registry.
    pub product_program_id: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<RegisterLookupTable>, lookup_table: Pubkey) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let registry = &mut ctx.accounts.lookup_table_registry;

    if registry.version == 0 {
        registry.version = LookupTableRegistry::CURRENT_VERSION;
        registry.product_program_id = ctx.accounts.product_program_id.key();
    }

    let idx = registry.count as usize;
    require!(
        idx < crate::state::lookup_table_registry::MAX_LOOKUP_TABLES,
        KernelError::LookupTableRegistryFull
    );
    registry.tables[idx] = lookup_table;
    registry.count = registry.count.saturating_add(1);
    registry.last_update_ts = now;
    Ok(())
}
