use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};

use crate::{state::*, KernelError};

#[derive(Accounts)]
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
