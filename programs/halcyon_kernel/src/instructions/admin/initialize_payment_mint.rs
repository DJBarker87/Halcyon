use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};
use halcyon_common::{events::ConfigUpdated, seeds, HalcyonError};

use crate::state::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct InitializePaymentMintArgs {
    /// When true, rotate sweep/defund destinations to the admin's ATA for
    /// this mint. Devnet mock-USDC bring-up should leave this true.
    pub set_admin_destinations: bool,
}

#[derive(Accounts)]
pub struct InitializePaymentMint<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [seeds::PROTOCOL_CONFIG],
        bump,
        has_one = admin @ HalcyonError::AdminMismatch,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    pub usdc_mint: Account<'info, Mint>,

    /// CHECK: PDA that owns every kernel-side token account.
    #[account(seeds = [seeds::VAULT_AUTHORITY], bump)]
    pub vault_authority: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = admin,
        token::mint = usdc_mint,
        token::authority = vault_authority,
        seeds = [seeds::VAULT_USDC, usdc_mint.key().as_ref()],
        bump,
    )]
    pub vault_usdc: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = admin,
        token::mint = usdc_mint,
        token::authority = vault_authority,
        seeds = [seeds::TREASURY_USDC, usdc_mint.key().as_ref()],
        bump,
    )]
    pub treasury_usdc: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = admin,
        associated_token::mint = usdc_mint,
        associated_token::authority = admin,
    )]
    pub admin_usdc: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<InitializePaymentMint>, args: InitializePaymentMintArgs) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let cfg = &mut ctx.accounts.protocol_config;

    if args.set_admin_destinations {
        cfg.treasury_destination = ctx.accounts.admin_usdc.key();
        cfg.hedge_defund_destination = ctx.accounts.admin_usdc.key();
    }
    cfg.last_update_ts = now;

    emit!(ConfigUpdated {
        admin: cfg.admin,
        field_tag: 1,
        updated_at: now,
    });

    Ok(())
}
