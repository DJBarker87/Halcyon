use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
use halcyon_common::{seeds, HalcyonError};

use crate::state::*;

#[derive(Accounts)]
pub struct SeedJunior<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    pub usdc_mint: Account<'info, Mint>,

    #[account(
        seeds = [seeds::PROTOCOL_CONFIG],
        bump,
        has_one = admin @ HalcyonError::AdminMismatch,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(mut, seeds = [seeds::VAULT_STATE], bump)]
    pub vault_state: Account<'info, VaultState>,

    #[account(mut, constraint = admin_usdc.mint == usdc_mint.key())]
    pub admin_usdc: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [seeds::VAULT_USDC, usdc_mint.key().as_ref()],
        bump,
        constraint = vault_usdc.mint == usdc_mint.key(),
    )]
    pub vault_usdc: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = admin,
        space = 8 + JuniorTranche::INIT_SPACE,
        seeds = [seeds::JUNIOR, admin.key().as_ref()],
        bump,
    )]
    pub junior: Account<'info, JuniorTranche>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<SeedJunior>, amount: u64) -> Result<()> {
    require!(amount > 0, HalcyonError::BelowMinimumTrade);

    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.admin_usdc.to_account_info(),
                to: ctx.accounts.vault_usdc.to_account_info(),
                authority: ctx.accounts.admin.to_account_info(),
            },
        ),
        amount,
    )?;

    let now = Clock::get()?.unix_timestamp;
    let junior = &mut ctx.accounts.junior;
    if junior.version == 0 {
        junior.version = JuniorTranche::CURRENT_VERSION;
        junior.owner = ctx.accounts.admin.key();
        junior.non_withdrawable = true;
        junior.created_ts = now;
    }
    junior.balance = junior
        .balance
        .checked_add(amount)
        .ok_or(HalcyonError::Overflow)?;

    let vault = &mut ctx.accounts.vault_state;
    vault.total_junior = vault
        .total_junior
        .checked_add(amount)
        .ok_or(HalcyonError::Overflow)?;
    vault.last_update_ts = now;
    Ok(())
}
