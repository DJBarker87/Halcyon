use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
use halcyon_common::{events::SleeveDefunded, seeds, HalcyonError};

use crate::state::*;

const HEDGE_SLEEVE_DEFUND_COOLDOWN_SECS: i64 = 86_400;

#[derive(Accounts)]
#[instruction(product_program_id: Pubkey)]
pub struct DefundHedgeSleeve<'info> {
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
        mut,
        seeds = [seeds::HEDGE_SLEEVE, product_program_id.as_ref()],
        bump,
        constraint = hedge_sleeve.product_program_id == product_program_id
            @ crate::KernelError::ProductProgramMismatch,
    )]
    pub hedge_sleeve: Account<'info, HedgeSleeve>,

    #[account(
        mut,
        constraint = hedge_sleeve_usdc.mint == usdc_mint.key(),
        constraint = hedge_sleeve_usdc.owner == hedge_sleeve.key()
            @ crate::KernelError::ProductProgramMismatch,
    )]
    pub hedge_sleeve_usdc: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = admin_usdc.mint == usdc_mint.key(),
        constraint = admin_usdc.owner == admin.key(),
    )]
    pub admin_usdc: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

pub fn handler(
    ctx: Context<DefundHedgeSleeve>,
    product_program_id: Pubkey,
    amount: u64,
) -> Result<()> {
    require!(amount > 0, HalcyonError::BelowMinimumTrade);

    let now = Clock::get()?.unix_timestamp;
    let last_defunded_ts = ctx.accounts.hedge_sleeve.last_defunded_ts;
    if last_defunded_ts != 0 {
        require!(
            now.saturating_sub(last_defunded_ts) >= HEDGE_SLEEVE_DEFUND_COOLDOWN_SECS,
            HalcyonError::CooldownNotElapsed
        );
    }

    let bump = ctx.bumps.hedge_sleeve;
    let signer_seeds: &[&[&[u8]]] = &[&[seeds::HEDGE_SLEEVE, product_program_id.as_ref(), &[bump]]];
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.hedge_sleeve_usdc.to_account_info(),
                to: ctx.accounts.admin_usdc.to_account_info(),
                authority: ctx.accounts.hedge_sleeve.to_account_info(),
            },
            signer_seeds,
        ),
        amount,
    )?;

    ctx.accounts.hedge_sleeve_usdc.reload()?;
    let hedge_sleeve = &mut ctx.accounts.hedge_sleeve;
    hedge_sleeve.usdc_reserve = ctx.accounts.hedge_sleeve_usdc.amount;
    hedge_sleeve.cumulative_defunded_usdc = hedge_sleeve
        .cumulative_defunded_usdc
        .checked_add(amount)
        .ok_or(HalcyonError::Overflow)?;
    hedge_sleeve.last_defunded_ts = now;
    hedge_sleeve.last_update_ts = now;

    emit!(SleeveDefunded {
        product_program_id,
        hedge_sleeve: hedge_sleeve.key(),
        amount,
        cumulative_defunded_usdc: hedge_sleeve.cumulative_defunded_usdc,
        defunded_at: now,
    });
    Ok(())
}
