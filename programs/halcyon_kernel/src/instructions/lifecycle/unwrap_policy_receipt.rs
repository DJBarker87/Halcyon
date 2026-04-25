use anchor_lang::prelude::*;
use anchor_spl::token::{self, Burn, Mint, Token, TokenAccount};
use halcyon_common::{events::PolicyReceiptUnwrapped, seeds, HalcyonError};

use crate::{state::policy::PolicyReceipt, state::*, KernelError};

#[derive(Accounts)]
pub struct UnwrapPolicyReceipt<'info> {
    #[account(mut)]
    pub holder: Signer<'info>,

    #[account(mut)]
    pub policy_header: Account<'info, PolicyHeader>,

    #[account(
        mut,
        close = holder,
        seeds = [seeds::POLICY_RECEIPT, policy_header.key().as_ref()],
        bump,
        constraint = policy_receipt.policy_header == policy_header.key() @ KernelError::PolicyReceiptMismatch,
        constraint = policy_receipt.product_program_id == policy_header.product_program_id @ KernelError::PolicyReceiptMismatch,
        constraint = policy_receipt.escrow_authority == receipt_authority.key() @ KernelError::PolicyReceiptMismatch,
        constraint = policy_receipt.receipt_mint == receipt_mint.key() @ KernelError::PolicyReceiptMismatch,
    )]
    pub policy_receipt: Account<'info, PolicyReceipt>,

    #[account(
        mut,
        seeds = [seeds::POLICY_RECEIPT_MINT, policy_header.key().as_ref()],
        bump,
        constraint = receipt_mint.decimals == 0 @ KernelError::PolicyReceiptTokenInvalid,
        constraint = receipt_mint.supply == 1 @ KernelError::PolicyReceiptSupplyInvalid,
    )]
    pub receipt_mint: Account<'info, Mint>,

    /// CHECK: PDA currently recorded as the policy owner.
    #[account(
        seeds = [seeds::POLICY_RECEIPT_AUTHORITY, policy_header.key().as_ref()],
        bump,
    )]
    pub receipt_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        constraint = holder_receipt_token.mint == receipt_mint.key() @ KernelError::PolicyReceiptTokenInvalid,
        constraint = holder_receipt_token.owner == holder.key() @ KernelError::PolicyReceiptTokenInvalid,
        constraint = holder_receipt_token.amount == 1 @ KernelError::PolicyReceiptTokenInvalid,
    )]
    pub holder_receipt_token: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

pub fn handler(ctx: Context<UnwrapPolicyReceipt>) -> Result<()> {
    require!(
        ctx.accounts.policy_header.status == PolicyStatus::Active,
        HalcyonError::PolicyNotActive
    );
    require_keys_eq!(
        ctx.accounts.policy_header.owner,
        ctx.accounts.receipt_authority.key(),
        KernelError::PolicyReceiptMismatch
    );

    token::burn(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Burn {
                mint: ctx.accounts.receipt_mint.to_account_info(),
                from: ctx.accounts.holder_receipt_token.to_account_info(),
                authority: ctx.accounts.holder.to_account_info(),
            },
        ),
        1,
    )?;

    let now = Clock::get()?.unix_timestamp;
    ctx.accounts.policy_header.owner = ctx.accounts.holder.key();

    emit!(PolicyReceiptUnwrapped {
        policy_id: ctx.accounts.policy_header.key(),
        product_program_id: ctx.accounts.policy_header.product_program_id,
        holder: ctx.accounts.holder.key(),
        receipt_mint: ctx.accounts.receipt_mint.key(),
        unwrapped_at: now,
    });

    Ok(())
}
