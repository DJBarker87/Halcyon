use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};

use crate::state::*;

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
        entry.global_risk_cap = x;
    }
    if let Some(x) = args.engine_version {
        entry.engine_version = x;
    }
    if let Some(d) = args.init_terms_discriminator {
        entry.init_terms_discriminator = d;
    }
    entry.last_update_ts = now;
    Ok(())
}
