use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};

use crate::{state::*, KernelError};

// L-3 — Address Lookup Table program ID. Hardcoded because anchor-lang
// 0.32.1's re-export surface does not expose it through
// `solana_program::address_lookup_table`. Value from
// <https://docs.solana.com/runtime/programs#address-lookup-table>.
const ADDRESS_LOOKUP_TABLE_PROGRAM_ID: Pubkey =
    pubkey!("AddressLookupTab1e1111111111111111111111111");

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

    /// CHECK: the lookup table account itself. Must be owned by the address
    /// lookup table program and match the `lookup_table` pubkey the admin is
    /// committing. L-3 — guards against typos or unintentional registration
    /// of non-ALT accounts.
    #[account(
        constraint = lookup_table_account.key() == lookup_table
            @ KernelError::LookupTableRegistryFull,
        constraint = *lookup_table_account.owner == ADDRESS_LOOKUP_TABLE_PROGRAM_ID
            @ KernelError::LookupTableRegistryFull,
    )]
    pub lookup_table_account: UncheckedAccount<'info>,

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
