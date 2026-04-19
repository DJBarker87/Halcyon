use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};

use crate::state::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct RegisterProductArgs {
    pub product_program_id: Pubkey,
    pub expected_authority: Pubkey,
    pub per_policy_risk_cap: u64,
    pub global_risk_cap: u64,
    pub engine_version: u16,
    pub init_terms_discriminator: [u8; 8],
}

#[derive(Accounts)]
#[instruction(args: RegisterProductArgs)]
pub struct RegisterProduct<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        seeds = [seeds::PROTOCOL_CONFIG],
        bump,
        has_one = admin @ HalcyonError::AdminMismatch,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(
        init,
        payer = admin,
        space = 8 + ProductRegistryEntry::INIT_SPACE,
        seeds = [seeds::PRODUCT_REGISTRY, args.product_program_id.as_ref()],
        bump,
    )]
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,

    #[account(
        init,
        payer = admin,
        space = 8 + VaultSigma::INIT_SPACE,
        seeds = [seeds::VAULT_SIGMA, args.product_program_id.as_ref()],
        bump,
    )]
    pub vault_sigma: Account<'info, VaultSigma>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<RegisterProduct>, args: RegisterProductArgs) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;

    // K15 — canonical product_authority PDA. If the admin passes any other
    // key, a subsequent issuance could be signed by a plain keypair without
    // `invoke_signed` and the kernel's program-origin guarantee collapses.
    let (canonical_authority, _bump) =
        Pubkey::find_program_address(&[seeds::PRODUCT_AUTHORITY], &args.product_program_id);
    require_keys_eq!(
        args.expected_authority,
        canonical_authority,
        HalcyonError::ProductAuthorityNotPda
    );

    let entry = &mut ctx.accounts.product_registry_entry;
    entry.version = ProductRegistryEntry::CURRENT_VERSION;
    entry.product_program_id = args.product_program_id;
    entry.expected_authority = args.expected_authority;
    entry.active = true;
    entry.paused = false;
    entry.per_policy_risk_cap = args.per_policy_risk_cap;
    entry.global_risk_cap = args.global_risk_cap;
    entry.engine_version = args.engine_version;
    entry.init_terms_discriminator = args.init_terms_discriminator;
    entry.total_reserved = 0;
    entry.last_update_ts = now;

    let sigma = &mut ctx.accounts.vault_sigma;
    sigma.version = VaultSigma::CURRENT_VERSION;
    sigma.product_program_id = args.product_program_id;
    sigma.ewma_last_timestamp = now;
    Ok(())
}
