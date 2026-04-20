use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Keypair, system_program};

use crate::{pda, tx};

pub use halcyon_il_protection::{AcceptQuoteArgs, QuotePreview};

pub fn preview_quote_ix(
    protocol_config: Pubkey,
    product_registry_entry: Pubkey,
    vault_sigma: Pubkey,
    regime_signal: Pubkey,
    pyth_sol: Pubkey,
    pyth_usdc: Pubkey,
    insured_notional_usdc: u64,
) -> Instruction {
    Instruction {
        program_id: halcyon_il_protection::ID,
        accounts: halcyon_il_protection::accounts::PreviewQuote {
            protocol_config,
            product_registry_entry,
            vault_sigma,
            regime_signal,
            pyth_sol,
            pyth_usdc,
            clock: solana_sdk::sysvar::clock::ID,
        }
        .to_account_metas(None),
        data: halcyon_il_protection::instruction::PreviewQuote {
            insured_notional_usdc,
        }
        .data(),
    }
}

pub async fn simulate_preview_quote(
    rpc: &RpcClient,
    payer: &Keypair,
    pyth_sol: Pubkey,
    pyth_usdc: Pubkey,
    insured_notional_usdc: u64,
) -> Result<QuotePreview> {
    let (protocol_config, _) = pda::protocol_config();
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_il_protection::ID);
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_il_protection::ID);
    let (regime_signal, _) = pda::regime_signal(&halcyon_il_protection::ID);
    let ix = preview_quote_ix(
        protocol_config,
        product_registry_entry,
        vault_sigma,
        regime_signal,
        pyth_sol,
        pyth_usdc,
        insured_notional_usdc,
    );
    let result = tx::simulate_instruction(rpc, payer, ix).await?;
    tx::decode_return_data(result, &halcyon_il_protection::ID)
}

pub fn accept_quote_ix(
    buyer: &Pubkey,
    usdc_mint: &Pubkey,
    pyth_sol: Pubkey,
    pyth_usdc: Pubkey,
    args: AcceptQuoteArgs,
) -> Instruction {
    let buyer_usdc = pda::associated_token_account(buyer, usdc_mint);
    let (policy_header, _) = pda::policy(&args.policy_id);
    let (product_terms, _) = pda::terms_for(&halcyon_il_protection::ID, &args.policy_id);
    let (product_authority, _) = pda::product_authority_for(&halcyon_il_protection::ID);
    let (vault_usdc, _) = pda::vault_usdc(usdc_mint);
    let (treasury_usdc, _) = pda::treasury_usdc(usdc_mint);
    let (vault_authority, _) = pda::vault_authority();
    let (protocol_config, _) = pda::protocol_config();
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_il_protection::ID);
    let (regime_signal, _) = pda::regime_signal(&halcyon_il_protection::ID);
    let (vault_state, _) = pda::vault_state();
    let (fee_ledger, _) = pda::fee_ledger();
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_il_protection::ID);

    Instruction {
        program_id: halcyon_il_protection::ID,
        accounts: halcyon_il_protection::accounts::AcceptQuote {
            buyer: *buyer,
            policy_header,
            product_terms,
            product_authority,
            usdc_mint: *usdc_mint,
            buyer_usdc,
            vault_usdc,
            treasury_usdc,
            vault_authority,
            protocol_config,
            vault_sigma,
            regime_signal,
            pyth_sol,
            pyth_usdc,
            vault_state,
            fee_ledger,
            product_registry_entry,
            clock: solana_sdk::sysvar::clock::ID,
            kernel_program: halcyon_kernel::ID,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: halcyon_il_protection::instruction::AcceptQuote { args }.data(),
    }
}

pub fn settle_ix(
    caller: &Pubkey,
    usdc_mint: &Pubkey,
    pyth_sol: Pubkey,
    pyth_usdc: Pubkey,
    policy: &halcyon_kernel::state::PolicyHeader,
    policy_address: Pubkey,
) -> Instruction {
    let buyer_usdc = pda::associated_token_account(&policy.owner, usdc_mint);
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_il_protection::ID);
    let (protocol_config, _) = pda::protocol_config();
    let (vault_usdc, _) = pda::vault_usdc(usdc_mint);
    let (vault_authority, _) = pda::vault_authority();
    let (product_authority, _) = pda::product_authority_for(&halcyon_il_protection::ID);
    let (vault_state, _) = pda::vault_state();
    Instruction {
        program_id: halcyon_il_protection::ID,
        accounts: halcyon_il_protection::accounts::Settle {
            caller: *caller,
            policy_header: policy_address,
            product_terms: policy.product_terms,
            product_registry_entry,
            protocol_config,
            pyth_sol,
            pyth_usdc,
            usdc_mint: *usdc_mint,
            vault_usdc,
            vault_authority,
            buyer_usdc,
            product_authority,
            vault_state,
            clock: solana_sdk::sysvar::clock::ID,
            kernel_program: halcyon_kernel::ID,
            token_program: anchor_spl::token::ID,
        }
        .to_account_metas(None),
        data: halcyon_il_protection::instruction::Settle {}.data(),
    }
}
