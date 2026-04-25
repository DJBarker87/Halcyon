use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, MintTo, Token, TokenAccount},
};
use halcyon_common::{events::PolicyReceiptWrapped, seeds, HalcyonError};

use crate::{state::policy::PolicyReceipt, state::*, KernelError};

#[derive(Accounts)]
pub struct WrapPolicyReceipt<'info> {
    #[account(mut)]
    pub current_owner: Signer<'info>,

    #[account(
        mut,
        constraint = policy_header.owner == current_owner.key() @ HalcyonError::ProductAuthorityMismatch,
    )]
    pub policy_header: Account<'info, PolicyHeader>,

    #[account(
        init,
        payer = current_owner,
        space = 8 + PolicyReceipt::INIT_SPACE,
        seeds = [seeds::POLICY_RECEIPT, policy_header.key().as_ref()],
        bump,
    )]
    pub policy_receipt: Account<'info, PolicyReceipt>,

    #[account(
        init,
        payer = current_owner,
        seeds = [seeds::POLICY_RECEIPT_MINT, policy_header.key().as_ref()],
        bump,
        mint::decimals = 0,
        mint::authority = receipt_authority,
        mint::freeze_authority = receipt_authority,
    )]
    pub receipt_mint: Account<'info, Mint>,

    /// CHECK: PDA authority that holds `PolicyHeader.owner` while the receipt
    /// token is outstanding, and signs the one-time mint.
    #[account(
        seeds = [seeds::POLICY_RECEIPT_AUTHORITY, policy_header.key().as_ref()],
        bump,
    )]
    pub receipt_authority: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = current_owner,
        associated_token::mint = receipt_mint,
        associated_token::authority = current_owner,
    )]
    pub holder_receipt_token: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<WrapPolicyReceipt>) -> Result<()> {
    require!(
        ctx.accounts.policy_header.status == PolicyStatus::Active,
        HalcyonError::PolicyNotActive
    );
    require!(
        ctx.accounts.receipt_mint.supply == 0,
        KernelError::PolicyReceiptSupplyInvalid
    );
    require!(
        ctx.accounts.holder_receipt_token.amount == 0,
        KernelError::PolicyReceiptTokenInvalid
    );

    let now = Clock::get()?.unix_timestamp;
    let receipt_authority = ctx.accounts.receipt_authority.key();

    ctx.accounts.policy_receipt.set_inner(PolicyReceipt {
        version: PolicyReceipt::CURRENT_VERSION,
        policy_header: ctx.accounts.policy_header.key(),
        product_program_id: ctx.accounts.policy_header.product_program_id,
        receipt_mint: ctx.accounts.receipt_mint.key(),
        escrow_authority: receipt_authority,
        wrapped_at: now,
    });
    ctx.accounts.policy_header.owner = receipt_authority;

    let bump = ctx.bumps.receipt_authority;
    let policy_key = ctx.accounts.policy_header.key();
    let signer_seeds: &[&[&[u8]]] = &[&[
        seeds::POLICY_RECEIPT_AUTHORITY,
        policy_key.as_ref(),
        &[bump],
    ]];
    token::mint_to(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            MintTo {
                mint: ctx.accounts.receipt_mint.to_account_info(),
                to: ctx.accounts.holder_receipt_token.to_account_info(),
                authority: ctx.accounts.receipt_authority.to_account_info(),
            },
            signer_seeds,
        ),
        1,
    )?;

    emit!(PolicyReceiptWrapped {
        policy_id: ctx.accounts.policy_header.key(),
        product_program_id: ctx.accounts.policy_header.product_program_id,
        holder: ctx.accounts.current_owner.key(),
        receipt_mint: ctx.accounts.receipt_mint.key(),
        escrow_authority: receipt_authority,
        wrapped_at: now,
    });

    Ok(())
}
