use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Keypair, system_program};

use crate::{pda, tx};

pub use halcyon_sol_autocall::{AcceptQuoteArgs, LendingValuePreview, QuotePreview};

pub fn preview_quote_ix(
    protocol_config: Pubkey,
    product_registry_entry: Pubkey,
    vault_sigma: Pubkey,
    regime_signal: Pubkey,
    reduced_operators: Pubkey,
    pyth_sol: Pubkey,
    notional: u64,
) -> Instruction {
    Instruction {
        program_id: halcyon_sol_autocall::ID,
        accounts: halcyon_sol_autocall::accounts::PreviewQuote {
            protocol_config,
            product_registry_entry,
            vault_sigma,
            regime_signal,
            reduced_operators,
            pyth_sol,
            clock: solana_sdk::sysvar::clock::ID,
        }
        .to_account_metas(None),
        data: halcyon_sol_autocall::instruction::PreviewQuote {
            notional_usdc: notional,
        }
        .data(),
    }
}

pub async fn simulate_preview_quote(
    rpc: &RpcClient,
    payer: &Keypair,
    pyth_sol: Pubkey,
    notional: u64,
) -> Result<QuotePreview> {
    let (protocol_config, _) = pda::protocol_config();
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_sol_autocall::ID);
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_sol_autocall::ID);
    let (regime_signal, _) = pda::regime_signal(&halcyon_sol_autocall::ID);
    let (reduced_operators, _) = pda::sol_autocall_reduced_operators();
    let ix = preview_quote_ix(
        protocol_config,
        product_registry_entry,
        vault_sigma,
        regime_signal,
        reduced_operators,
        pyth_sol,
        notional,
    );
    let result = tx::simulate_instruction(rpc, payer, ix).await?;
    tx::decode_return_data(result, &halcyon_sol_autocall::ID)
}

pub fn preview_lending_value_ix(
    protocol_config: Pubkey,
    vault_sigma: Pubkey,
    regime_signal: Pubkey,
    policy_header: Pubkey,
    product_terms: Pubkey,
    midlife_matrices: Pubkey,
    pyth_sol: Pubkey,
) -> Instruction {
    Instruction {
        program_id: halcyon_sol_autocall::ID,
        accounts: halcyon_sol_autocall::accounts::PreviewLendingValue {
            protocol_config,
            vault_sigma,
            regime_signal,
            policy_header,
            product_terms,
            midlife_matrices,
            pyth_sol,
            clock: solana_sdk::sysvar::clock::ID,
        }
        .to_account_metas(None),
        data: halcyon_sol_autocall::instruction::PreviewLendingValue {}.data(),
    }
}

pub async fn simulate_preview_lending_value(
    rpc: &RpcClient,
    payer: &Keypair,
    pyth_sol: Pubkey,
    policy: &halcyon_kernel::state::PolicyHeader,
    policy_address: Pubkey,
) -> Result<LendingValuePreview> {
    let (protocol_config, _) = pda::protocol_config();
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_sol_autocall::ID);
    let (regime_signal, _) = pda::regime_signal(&halcyon_sol_autocall::ID);
    let (midlife_matrices, _) = pda::sol_autocall_midlife_matrices();
    let ix = preview_lending_value_ix(
        protocol_config,
        vault_sigma,
        regime_signal,
        policy_address,
        policy.product_terms,
        midlife_matrices,
        pyth_sol,
    );
    let result = tx::simulate_instruction(rpc, payer, ix).await?;
    tx::decode_return_data(result, &halcyon_sol_autocall::ID)
}

pub fn accept_quote_ix(
    buyer: &Pubkey,
    usdc_mint: &Pubkey,
    pyth_sol: Pubkey,
    args: AcceptQuoteArgs,
) -> Instruction {
    let buyer_usdc = pda::associated_token_account(buyer, usdc_mint);
    let (policy_header, _) = pda::policy(&args.policy_id);
    let (product_terms, _) = pda::terms(&args.policy_id);
    let (product_authority, _) = pda::product_authority();
    let (vault_usdc, _) = pda::vault_usdc(usdc_mint);
    let (treasury_usdc, _) = pda::treasury_usdc(usdc_mint);
    let (vault_authority, _) = pda::vault_authority();
    let (protocol_config, _) = pda::protocol_config();
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_sol_autocall::ID);
    let (regime_signal, _) = pda::regime_signal(&halcyon_sol_autocall::ID);
    let (reduced_operators, _) = pda::sol_autocall_reduced_operators();
    let (vault_state, _) = pda::vault_state();
    let (fee_ledger, _) = pda::fee_ledger();
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_sol_autocall::ID);

    Instruction {
        program_id: halcyon_sol_autocall::ID,
        accounts: halcyon_sol_autocall::accounts::AcceptQuote {
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
            reduced_operators,
            pyth_sol,
            vault_state,
            fee_ledger,
            product_registry_entry,
            clock: solana_sdk::sysvar::clock::ID,
            kernel_program: halcyon_kernel::ID,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: halcyon_sol_autocall::instruction::AcceptQuote { args }.data(),
    }
}

pub fn write_reduced_operators_ix(
    keeper: &Pubkey,
    args: halcyon_sol_autocall::WriteReducedOperatorsArgs,
) -> Instruction {
    let (protocol_config, _) = pda::protocol_config();
    let (keeper_registry, _) = pda::keeper_registry();
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_sol_autocall::ID);
    let (regime_signal, _) = pda::regime_signal(&halcyon_sol_autocall::ID);
    let (reduced_operators, _) = pda::sol_autocall_reduced_operators();

    Instruction {
        program_id: halcyon_sol_autocall::ID,
        accounts: halcyon_sol_autocall::accounts::WriteReducedOperators {
            keeper: *keeper,
            protocol_config,
            keeper_registry,
            vault_sigma,
            regime_signal,
            reduced_operators,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: halcyon_sol_autocall::instruction::WriteReducedOperators { args }.data(),
    }
}

pub fn settle_ix(
    caller: &Pubkey,
    usdc_mint: &Pubkey,
    pyth_sol: Pubkey,
    policy: &halcyon_kernel::state::PolicyHeader,
    policy_address: Pubkey,
) -> Instruction {
    let buyer_usdc = pda::associated_token_account(&policy.owner, usdc_mint);
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_sol_autocall::ID);
    let (protocol_config, _) = pda::protocol_config();
    let (coupon_vault, _) = pda::coupon_vault(&halcyon_sol_autocall::ID);
    let coupon_vault_usdc = pda::associated_token_account(&coupon_vault, usdc_mint);
    let (vault_usdc, _) = pda::vault_usdc(usdc_mint);
    let (vault_authority, _) = pda::vault_authority();
    let (product_authority, _) = pda::product_authority();
    let (vault_state, _) = pda::vault_state();
    Instruction {
        program_id: halcyon_sol_autocall::ID,
        accounts: halcyon_sol_autocall::accounts::Settle {
            caller: *caller,
            policy_header: policy_address,
            product_terms: policy.product_terms,
            product_registry_entry,
            protocol_config,
            pyth_sol,
            usdc_mint: *usdc_mint,
            coupon_vault,
            coupon_vault_usdc,
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
        data: halcyon_sol_autocall::instruction::Settle {}.data(),
    }
}

pub fn record_observation_ix(
    keeper: &Pubkey,
    usdc_mint: &Pubkey,
    pyth_sol: Pubkey,
    policy: &halcyon_kernel::state::PolicyHeader,
    policy_address: Pubkey,
    expected_index: u8,
) -> Instruction {
    let buyer_usdc = pda::associated_token_account(&policy.owner, usdc_mint);
    let (keeper_registry, _) = pda::keeper_registry();
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_sol_autocall::ID);
    let (protocol_config, _) = pda::protocol_config();
    let (coupon_vault, _) = pda::coupon_vault(&halcyon_sol_autocall::ID);
    let coupon_vault_usdc = pda::associated_token_account(&coupon_vault, usdc_mint);
    let (vault_usdc, _) = pda::vault_usdc(usdc_mint);
    let (vault_authority, _) = pda::vault_authority();
    let (product_authority, _) = pda::product_authority();
    let (vault_state, _) = pda::vault_state();
    Instruction {
        program_id: halcyon_sol_autocall::ID,
        accounts: halcyon_sol_autocall::accounts::RecordObservation {
            keeper: *keeper,
            keeper_registry,
            policy_header: policy_address,
            product_terms: policy.product_terms,
            product_registry_entry,
            protocol_config,
            pyth_sol,
            usdc_mint: *usdc_mint,
            coupon_vault,
            coupon_vault_usdc,
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
        data: halcyon_sol_autocall::instruction::RecordObservation { expected_index }.data(),
    }
}
