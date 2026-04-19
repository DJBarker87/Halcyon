use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, Token, TokenAccount, Transfer},
};
use halcyon_common::{events::SleeveFunded, seeds, HalcyonError};

use crate::state::*;

#[derive(Accounts)]
#[instruction(product_program_id: Pubkey)]
pub struct FundHedgeSleeve<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    pub usdc_mint: Account<'info, Mint>,

    #[account(
        seeds = [seeds::PROTOCOL_CONFIG],
        bump,
        has_one = admin @ HalcyonError::AdminMismatch,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(
        seeds = [seeds::PRODUCT_REGISTRY, product_program_id.as_ref()],
        bump,
        constraint = product_registry_entry.product_program_id == product_program_id
            @ crate::KernelError::ProductProgramMismatch,
        constraint = product_registry_entry.active @ HalcyonError::ProductNotRegistered,
    )]
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,

    #[account(
        init_if_needed,
        payer = admin,
        space = 8 + HedgeSleeve::INIT_SPACE,
        seeds = [seeds::HEDGE_SLEEVE, product_program_id.as_ref()],
        bump,
    )]
    pub hedge_sleeve: Account<'info, HedgeSleeve>,

    #[account(
        mut,
        constraint = admin_usdc.mint == usdc_mint.key(),
        constraint = admin_usdc.owner == admin.key(),
    )]
    pub admin_usdc: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = admin,
        associated_token::mint = usdc_mint,
        associated_token::authority = hedge_sleeve,
    )]
    pub hedge_sleeve_usdc: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<FundHedgeSleeve>,
    product_program_id: Pubkey,
    amount: u64,
) -> Result<()> {
    require!(amount > 0, HalcyonError::BelowMinimumTrade);

    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.admin_usdc.to_account_info(),
                to: ctx.accounts.hedge_sleeve_usdc.to_account_info(),
                authority: ctx.accounts.admin.to_account_info(),
            },
        ),
        amount,
    )?;

    let now = Clock::get()?.unix_timestamp;
    ctx.accounts.hedge_sleeve_usdc.reload()?;
    let hedge_sleeve = &mut ctx.accounts.hedge_sleeve;
    if hedge_sleeve.version == 0 {
        hedge_sleeve.version = HedgeSleeve::CURRENT_VERSION;
        hedge_sleeve.product_program_id = product_program_id;
    }
    require_keys_eq!(
        hedge_sleeve.product_program_id,
        product_program_id,
        crate::KernelError::ProductProgramMismatch
    );
    hedge_sleeve.usdc_reserve = ctx.accounts.hedge_sleeve_usdc.amount;
    hedge_sleeve.cumulative_funded_usdc = hedge_sleeve
        .cumulative_funded_usdc
        .checked_add(amount)
        .ok_or(HalcyonError::Overflow)?;
    hedge_sleeve.last_funded_ts = now;
    hedge_sleeve.last_update_ts = now;

    emit!(SleeveFunded {
        product_program_id,
        hedge_sleeve: hedge_sleeve.key(),
        amount,
        cumulative_funded_usdc: hedge_sleeve.cumulative_funded_usdc,
        funded_at: now,
    });
    Ok(())
}
