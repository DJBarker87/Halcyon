use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};

use crate::{state::*, KernelError};

// L-3 — same ALT program owner check as the initial registration path.
const ADDRESS_LOOKUP_TABLE_PROGRAM_ID: Pubkey =
    pubkey!("AddressLookupTab1e1111111111111111111111111");

#[derive(Accounts)]
#[instruction(_index: u8, new_lookup_table: Pubkey)]
pub struct UpdateLookupTable<'info> {
    pub admin: Signer<'info>,

    #[account(
        seeds = [seeds::PROTOCOL_CONFIG],
        bump,
        has_one = admin @ HalcyonError::AdminMismatch,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(
        mut,
        seeds = [seeds::ALT_REGISTRY, lookup_table_registry.product_program_id.as_ref()],
        bump,
    )]
    pub lookup_table_registry: Account<'info, LookupTableRegistry>,

    /// CHECK: owner and key are validated below.
    #[account(
        constraint = lookup_table_account.key() == new_lookup_table
            @ KernelError::InvalidLookupTableAccount,
        constraint = *lookup_table_account.owner == ADDRESS_LOOKUP_TABLE_PROGRAM_ID
            @ KernelError::InvalidLookupTableAccount,
    )]
    pub lookup_table_account: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<UpdateLookupTable>, index: u8, new_lookup_table: Pubkey) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let registry = &mut ctx.accounts.lookup_table_registry;
    require!(
        (index as usize) < registry.count as usize,
        KernelError::LookupTableIndexOutOfRange
    );
    registry.tables[index as usize] = new_lookup_table;
    registry.last_update_ts = now;
    Ok(())
}
