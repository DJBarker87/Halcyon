use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    instruction::Instruction, pubkey::Pubkey, signature::Keypair, system_instruction,
    system_program,
};

use crate::{pda, tx};

pub use halcyon_flagship_autocall::{
    AcceptPreparedQuoteArgs, AcceptQuoteArgs, FlagshipQuoteReceipt, LendingValuePreview,
    MidlifeNavCheckpointPreview, PrepareQuoteArgs, PreparedQuotePreview, QuotePreview,
};

pub const MIDLIFE_NAV_CHECKPOINT_ACCOUNT_SPACE: usize =
    halcyon_flagship_autocall::midlife_pricing::MIDLIFE_NAV_CHECKPOINT_ACCOUNT_SPACE;
pub const MIDLIFE_CHECKPOINT_TARGET_UNITS: u64 = 1_280_000;
pub const MIDLIFE_CHECKPOINT_MAX_CHUNK_SIZE: u8 = 18;
pub const MIDLIFE_FINAL_COUPON_INDEX: u8 = 18;

pub fn checkpoint_chunk_candidates(max_chunk_size: u8) -> Vec<u8> {
    let mut candidates = vec![max_chunk_size, 12, 9, 6, 4, 3, 2, 1];
    candidates.retain(|chunk| *chunk >= 1 && *chunk <= max_chunk_size);
    candidates.sort_by(|lhs, rhs| rhs.cmp(lhs));
    candidates.dedup();
    candidates
}

pub fn next_midlife_checkpoint_stop(current_coupon_index: u8, chunk_size: u8) -> u8 {
    current_coupon_index
        .saturating_add(chunk_size)
        .min(MIDLIFE_FINAL_COUPON_INDEX)
}

pub fn preview_quote_ix(
    protocol_config: Pubkey,
    product_registry_entry: Pubkey,
    vault_sigma: Pubkey,
    regression: Pubkey,
    autocall_schedule: Pubkey,
    pyth_spy: Pubkey,
    pyth_qqq: Pubkey,
    pyth_iwm: Pubkey,
    notional: u64,
) -> Instruction {
    Instruction {
        program_id: halcyon_flagship_autocall::ID,
        accounts: halcyon_flagship_autocall::accounts::PreviewQuote {
            protocol_config,
            product_registry_entry,
            vault_sigma,
            regression,
            autocall_schedule,
            pyth_spy,
            pyth_qqq,
            pyth_iwm,
            clock: solana_sdk::sysvar::clock::ID,
        }
        .to_account_metas(None),
        data: halcyon_flagship_autocall::instruction::PreviewQuote {
            notional_usdc: notional,
        }
        .data(),
    }
}

pub async fn simulate_preview_quote(
    rpc: &RpcClient,
    payer: &Keypair,
    pyth_spy: Pubkey,
    pyth_qqq: Pubkey,
    pyth_iwm: Pubkey,
    notional: u64,
) -> Result<QuotePreview> {
    let (protocol_config, _) = pda::protocol_config();
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_flagship_autocall::ID);
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_flagship_autocall::ID);
    let (regression, _) = pda::regression();
    let (autocall_schedule, _) = pda::autocall_schedule(&halcyon_flagship_autocall::ID);
    let ix = preview_quote_ix(
        protocol_config,
        product_registry_entry,
        vault_sigma,
        regression,
        autocall_schedule,
        pyth_spy,
        pyth_qqq,
        pyth_iwm,
        notional,
    );
    let result = tx::simulate_instruction(rpc, payer, ix).await?;
    tx::decode_return_data(result, &halcyon_flagship_autocall::ID)
}

pub fn prepare_quote_ix(
    buyer: &Pubkey,
    quote_receipt: Pubkey,
    pyth_spy: Pubkey,
    pyth_qqq: Pubkey,
    pyth_iwm: Pubkey,
    args: PrepareQuoteArgs,
) -> Instruction {
    let (protocol_config, _) = pda::protocol_config();
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_flagship_autocall::ID);
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_flagship_autocall::ID);
    let (regression, _) = pda::regression();
    let (autocall_schedule, _) = pda::autocall_schedule(&halcyon_flagship_autocall::ID);
    Instruction {
        program_id: halcyon_flagship_autocall::ID,
        accounts: halcyon_flagship_autocall::accounts::PrepareQuote {
            buyer: *buyer,
            quote_receipt,
            protocol_config,
            product_registry_entry,
            vault_sigma,
            regression,
            autocall_schedule,
            pyth_spy,
            pyth_qqq,
            pyth_iwm,
            clock: solana_sdk::sysvar::clock::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: halcyon_flagship_autocall::instruction::PrepareQuote { args }.data(),
    }
}

pub fn preview_lending_value_ix(
    protocol_config: Pubkey,
    vault_sigma: Pubkey,
    regression: Pubkey,
    policy_header: Pubkey,
    product_terms: Pubkey,
    pyth_spy: Pubkey,
    pyth_qqq: Pubkey,
    pyth_iwm: Pubkey,
) -> Instruction {
    Instruction {
        program_id: halcyon_flagship_autocall::ID,
        accounts: halcyon_flagship_autocall::accounts::PreviewLendingValue {
            protocol_config,
            vault_sigma,
            regression,
            policy_header,
            product_terms,
            pyth_spy,
            pyth_qqq,
            pyth_iwm,
            clock: solana_sdk::sysvar::clock::ID,
        }
        .to_account_metas(None),
        data: halcyon_flagship_autocall::instruction::PreviewLendingValue {}.data(),
    }
}

pub async fn simulate_preview_lending_value(
    rpc: &RpcClient,
    payer: &Keypair,
    pyth_spy: Pubkey,
    pyth_qqq: Pubkey,
    pyth_iwm: Pubkey,
    policy: &halcyon_kernel::state::PolicyHeader,
    policy_address: Pubkey,
) -> Result<LendingValuePreview> {
    let (protocol_config, _) = pda::protocol_config();
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_flagship_autocall::ID);
    let (regression, _) = pda::regression();
    let ix = preview_lending_value_ix(
        protocol_config,
        vault_sigma,
        regression,
        policy_address,
        policy.product_terms,
        pyth_spy,
        pyth_qqq,
        pyth_iwm,
    );
    let result = tx::simulate_instruction(rpc, payer, ix).await?;
    tx::decode_return_data(result, &halcyon_flagship_autocall::ID)
}

pub fn create_midlife_checkpoint_account_ix(
    payer: &Pubkey,
    checkpoint: &Pubkey,
    lamports: u64,
) -> Instruction {
    system_instruction::create_account(
        payer,
        checkpoint,
        lamports,
        MIDLIFE_NAV_CHECKPOINT_ACCOUNT_SPACE as u64,
        &halcyon_flagship_autocall::ID,
    )
}

pub fn prepare_midlife_nav_ix(
    requester: &Pubkey,
    checkpoint: Pubkey,
    pyth_spy: Pubkey,
    pyth_qqq: Pubkey,
    pyth_iwm: Pubkey,
    policy: &halcyon_kernel::state::PolicyHeader,
    policy_address: Pubkey,
    stop_coupon_index: u8,
) -> Instruction {
    let (protocol_config, _) = pda::protocol_config();
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_flagship_autocall::ID);
    let (regression, _) = pda::regression();
    Instruction {
        program_id: halcyon_flagship_autocall::ID,
        accounts: halcyon_flagship_autocall::accounts::PrepareMidlifeNav {
            requester: *requester,
            midlife_checkpoint: checkpoint,
            protocol_config,
            vault_sigma,
            regression,
            policy_header: policy_address,
            product_terms: policy.product_terms,
            pyth_spy,
            pyth_qqq,
            pyth_iwm,
            clock: solana_sdk::sysvar::clock::ID,
        }
        .to_account_metas(None),
        data: halcyon_flagship_autocall::instruction::PrepareMidlifeNav { stop_coupon_index }
            .data(),
    }
}

pub fn advance_midlife_nav_ix(
    requester: &Pubkey,
    checkpoint: Pubkey,
    policy: &halcyon_kernel::state::PolicyHeader,
    policy_address: Pubkey,
    stop_coupon_index: u8,
) -> Instruction {
    Instruction {
        program_id: halcyon_flagship_autocall::ID,
        accounts: halcyon_flagship_autocall::accounts::AdvanceMidlifeNav {
            requester: *requester,
            midlife_checkpoint: checkpoint,
            policy_header: policy_address,
            product_terms: policy.product_terms,
            clock: solana_sdk::sysvar::clock::ID,
        }
        .to_account_metas(None),
        data: halcyon_flagship_autocall::instruction::AdvanceMidlifeNav { stop_coupon_index }
            .data(),
    }
}

pub fn preview_lending_value_from_checkpoint_ix(
    requester: &Pubkey,
    checkpoint: Pubkey,
    policy: &halcyon_kernel::state::PolicyHeader,
    policy_address: Pubkey,
) -> Instruction {
    Instruction {
        program_id: halcyon_flagship_autocall::ID,
        accounts: halcyon_flagship_autocall::accounts::PreviewLendingValueFromCheckpoint {
            requester: *requester,
            midlife_checkpoint: checkpoint,
            policy_header: policy_address,
            product_terms: policy.product_terms,
            clock: solana_sdk::sysvar::clock::ID,
        }
        .to_account_metas(None),
        data: halcyon_flagship_autocall::instruction::PreviewLendingValueFromCheckpoint {}.data(),
    }
}

pub fn buyback_from_checkpoint_ix(
    policy_owner: &Pubkey,
    usdc_mint: &Pubkey,
    checkpoint: Pubkey,
    policy: &halcyon_kernel::state::PolicyHeader,
    policy_address: Pubkey,
) -> Instruction {
    let owner_usdc = pda::associated_token_account(policy_owner, usdc_mint);
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_flagship_autocall::ID);
    let (protocol_config, _) = pda::protocol_config();
    let (vault_usdc, _) = pda::vault_usdc(usdc_mint);
    let (vault_authority, _) = pda::vault_authority();
    let (product_authority, _) = pda::product_authority_for(&halcyon_flagship_autocall::ID);
    let (vault_state, _) = pda::vault_state();
    Instruction {
        program_id: halcyon_flagship_autocall::ID,
        accounts: halcyon_flagship_autocall::accounts::BuybackFromCheckpoint {
            policy_owner: *policy_owner,
            midlife_checkpoint: checkpoint,
            policy_header: policy_address,
            product_terms: policy.product_terms,
            product_registry_entry,
            protocol_config,
            usdc_mint: *usdc_mint,
            vault_usdc,
            vault_authority,
            owner_usdc,
            product_authority,
            vault_state,
            clock: solana_sdk::sysvar::clock::ID,
            kernel_program: halcyon_kernel::ID,
            token_program: anchor_spl::token::ID,
        }
        .to_account_metas(None),
        data: halcyon_flagship_autocall::instruction::BuybackFromCheckpoint {}.data(),
    }
}

pub fn liquidate_wrapped_flagship_from_checkpoint_ixs(
    holder: &Pubkey,
    usdc_mint: &Pubkey,
    checkpoint: Pubkey,
    policy: &halcyon_kernel::state::PolicyHeader,
    policy_address: Pubkey,
) -> Vec<Instruction> {
    let unwrap_ix = crate::kernel::unwrap_policy_receipt_ix(holder, &policy_address);
    let buyback_ix =
        buyback_from_checkpoint_ix(holder, usdc_mint, checkpoint, policy, policy_address);
    vec![unwrap_ix, buyback_ix]
}

pub fn accept_quote_ix(
    buyer: &Pubkey,
    usdc_mint: &Pubkey,
    pyth_spy: Pubkey,
    pyth_qqq: Pubkey,
    pyth_iwm: Pubkey,
    args: AcceptQuoteArgs,
) -> Instruction {
    let buyer_usdc = pda::associated_token_account(buyer, usdc_mint);
    let (policy_header, _) = pda::policy(&args.policy_id);
    let (product_terms, _) = pda::terms_for(&halcyon_flagship_autocall::ID, &args.policy_id);
    let (product_authority, _) = pda::product_authority_for(&halcyon_flagship_autocall::ID);
    let (vault_usdc, _) = pda::vault_usdc(usdc_mint);
    let (treasury_usdc, _) = pda::treasury_usdc(usdc_mint);
    let (vault_authority, _) = pda::vault_authority();
    let (protocol_config, _) = pda::protocol_config();
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_flagship_autocall::ID);
    let (regression, _) = pda::regression();
    let (autocall_schedule, _) = pda::autocall_schedule(&halcyon_flagship_autocall::ID);
    let (coupon_schedule, _) = pda::coupon_schedule(&halcyon_flagship_autocall::ID);
    let (vault_state, _) = pda::vault_state();
    let (fee_ledger, _) = pda::fee_ledger();
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_flagship_autocall::ID);

    Instruction {
        program_id: halcyon_flagship_autocall::ID,
        accounts: halcyon_flagship_autocall::accounts::AcceptQuote {
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
            regression,
            autocall_schedule,
            coupon_schedule,
            pyth_spy,
            pyth_qqq,
            pyth_iwm,
            vault_state,
            fee_ledger,
            product_registry_entry,
            clock: solana_sdk::sysvar::clock::ID,
            kernel_program: halcyon_kernel::ID,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: halcyon_flagship_autocall::instruction::AcceptQuote { args }.data(),
    }
}

pub fn accept_prepared_quote_ix(
    buyer: &Pubkey,
    usdc_mint: &Pubkey,
    quote_receipt: Pubkey,
    pyth_spy: Pubkey,
    pyth_qqq: Pubkey,
    pyth_iwm: Pubkey,
    args: AcceptPreparedQuoteArgs,
    receipt: &FlagshipQuoteReceipt,
) -> Instruction {
    let buyer_usdc = pda::associated_token_account(buyer, usdc_mint);
    let (policy_header, _) = pda::policy(&receipt.policy_id);
    let (product_terms, _) = pda::terms_for(&halcyon_flagship_autocall::ID, &receipt.policy_id);
    let (product_authority, _) = pda::product_authority_for(&halcyon_flagship_autocall::ID);
    let (vault_usdc, _) = pda::vault_usdc(usdc_mint);
    let (treasury_usdc, _) = pda::treasury_usdc(usdc_mint);
    let (vault_authority, _) = pda::vault_authority();
    let (protocol_config, _) = pda::protocol_config();
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_flagship_autocall::ID);
    let (autocall_schedule, _) = pda::autocall_schedule(&halcyon_flagship_autocall::ID);
    let (coupon_schedule, _) = pda::coupon_schedule(&halcyon_flagship_autocall::ID);
    let (vault_state, _) = pda::vault_state();
    let (fee_ledger, _) = pda::fee_ledger();
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_flagship_autocall::ID);

    Instruction {
        program_id: halcyon_flagship_autocall::ID,
        accounts: halcyon_flagship_autocall::accounts::AcceptPreparedQuote {
            buyer: *buyer,
            quote_receipt,
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
            autocall_schedule,
            coupon_schedule,
            pyth_spy,
            pyth_qqq,
            pyth_iwm,
            vault_state,
            fee_ledger,
            product_registry_entry,
            clock: solana_sdk::sysvar::clock::ID,
            kernel_program: halcyon_kernel::ID,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: halcyon_flagship_autocall::instruction::AcceptPreparedQuote { args }.data(),
    }
}

pub fn settle_ix(
    caller: &Pubkey,
    usdc_mint: &Pubkey,
    pyth_spy: Pubkey,
    pyth_qqq: Pubkey,
    pyth_iwm: Pubkey,
    policy: &halcyon_kernel::state::PolicyHeader,
    policy_address: Pubkey,
) -> Instruction {
    let buyer_usdc = pda::associated_token_account(&policy.owner, usdc_mint);
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_flagship_autocall::ID);
    let (protocol_config, _) = pda::protocol_config();
    let (coupon_vault, _) = pda::coupon_vault(&halcyon_flagship_autocall::ID);
    let coupon_vault_usdc = pda::associated_token_account(&coupon_vault, usdc_mint);
    let (vault_usdc, _) = pda::vault_usdc(usdc_mint);
    let (vault_authority, _) = pda::vault_authority();
    let (product_authority, _) = pda::product_authority_for(&halcyon_flagship_autocall::ID);
    let (vault_state, _) = pda::vault_state();
    Instruction {
        program_id: halcyon_flagship_autocall::ID,
        accounts: halcyon_flagship_autocall::accounts::Settle {
            caller: *caller,
            policy_header: policy_address,
            product_terms: policy.product_terms,
            product_registry_entry,
            protocol_config,
            pyth_spy,
            pyth_qqq,
            pyth_iwm,
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
        data: halcyon_flagship_autocall::instruction::Settle {}.data(),
    }
}

pub fn buyback_ix(
    policy_owner: &Pubkey,
    usdc_mint: &Pubkey,
    pyth_spy: Pubkey,
    pyth_qqq: Pubkey,
    pyth_iwm: Pubkey,
    policy: &halcyon_kernel::state::PolicyHeader,
    policy_address: Pubkey,
) -> Instruction {
    let owner_usdc = pda::associated_token_account(policy_owner, usdc_mint);
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_flagship_autocall::ID);
    let (protocol_config, _) = pda::protocol_config();
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_flagship_autocall::ID);
    let (regression, _) = pda::regression();
    let (vault_usdc, _) = pda::vault_usdc(usdc_mint);
    let (vault_authority, _) = pda::vault_authority();
    let (product_authority, _) = pda::product_authority_for(&halcyon_flagship_autocall::ID);
    let (vault_state, _) = pda::vault_state();
    Instruction {
        program_id: halcyon_flagship_autocall::ID,
        accounts: halcyon_flagship_autocall::accounts::Buyback {
            policy_owner: *policy_owner,
            policy_header: policy_address,
            product_terms: policy.product_terms,
            product_registry_entry,
            protocol_config,
            vault_sigma,
            regression,
            pyth_spy,
            pyth_qqq,
            pyth_iwm,
            usdc_mint: *usdc_mint,
            vault_usdc,
            vault_authority,
            owner_usdc,
            product_authority,
            vault_state,
            clock: solana_sdk::sysvar::clock::ID,
            kernel_program: halcyon_kernel::ID,
            token_program: anchor_spl::token::ID,
        }
        .to_account_metas(None),
        data: halcyon_flagship_autocall::instruction::Buyback {}.data(),
    }
}

pub fn request_retail_redemption_ix(
    policy_owner: &Pubkey,
    policy: &halcyon_kernel::state::PolicyHeader,
    policy_address: Pubkey,
) -> Instruction {
    let (redemption_request, _) = pda::retail_redemption_request(&policy_address);
    Instruction {
        program_id: halcyon_flagship_autocall::ID,
        accounts: halcyon_flagship_autocall::accounts::RequestRetailRedemption {
            policy_owner: *policy_owner,
            policy_header: policy_address,
            product_terms: policy.product_terms,
            redemption_request,
            clock: solana_sdk::sysvar::clock::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: halcyon_flagship_autocall::instruction::RequestRetailRedemption {}.data(),
    }
}

pub fn cancel_retail_redemption_ix(
    policy_owner: &Pubkey,
    policy: &halcyon_kernel::state::PolicyHeader,
    policy_address: Pubkey,
) -> Instruction {
    let (redemption_request, _) = pda::retail_redemption_request(&policy_address);
    Instruction {
        program_id: halcyon_flagship_autocall::ID,
        accounts: halcyon_flagship_autocall::accounts::CancelRetailRedemption {
            policy_owner: *policy_owner,
            policy_header: policy_address,
            product_terms: policy.product_terms,
            redemption_request,
            clock: solana_sdk::sysvar::clock::ID,
        }
        .to_account_metas(None),
        data: halcyon_flagship_autocall::instruction::CancelRetailRedemption {}.data(),
    }
}

pub fn execute_retail_redemption_ix(
    policy_owner: &Pubkey,
    usdc_mint: &Pubkey,
    pyth_spy: Pubkey,
    pyth_qqq: Pubkey,
    pyth_iwm: Pubkey,
    policy: &halcyon_kernel::state::PolicyHeader,
    policy_address: Pubkey,
) -> Instruction {
    let owner_usdc = pda::associated_token_account(policy_owner, usdc_mint);
    let (redemption_request, _) = pda::retail_redemption_request(&policy_address);
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_flagship_autocall::ID);
    let (protocol_config, _) = pda::protocol_config();
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_flagship_autocall::ID);
    let (regression, _) = pda::regression();
    let (vault_usdc, _) = pda::vault_usdc(usdc_mint);
    let (vault_authority, _) = pda::vault_authority();
    let (product_authority, _) = pda::product_authority_for(&halcyon_flagship_autocall::ID);
    let (vault_state, _) = pda::vault_state();
    Instruction {
        program_id: halcyon_flagship_autocall::ID,
        accounts: halcyon_flagship_autocall::accounts::ExecuteRetailRedemption {
            policy_owner: *policy_owner,
            policy_header: policy_address,
            product_terms: policy.product_terms,
            redemption_request,
            product_registry_entry,
            protocol_config,
            vault_sigma,
            regression,
            pyth_spy,
            pyth_qqq,
            pyth_iwm,
            usdc_mint: *usdc_mint,
            vault_usdc,
            vault_authority,
            owner_usdc,
            product_authority,
            vault_state,
            clock: solana_sdk::sysvar::clock::ID,
            kernel_program: halcyon_kernel::ID,
            token_program: anchor_spl::token::ID,
        }
        .to_account_metas(None),
        data: halcyon_flagship_autocall::instruction::ExecuteRetailRedemption {}.data(),
    }
}

pub fn liquidate_wrapped_flagship_ixs(
    holder: &Pubkey,
    usdc_mint: &Pubkey,
    pyth_spy: Pubkey,
    pyth_qqq: Pubkey,
    pyth_iwm: Pubkey,
    policy: &halcyon_kernel::state::PolicyHeader,
    policy_address: Pubkey,
) -> Vec<Instruction> {
    let unwrap_ix = crate::kernel::unwrap_policy_receipt_ix(holder, &policy_address);
    let buyback_ix = buyback_ix(
        holder,
        usdc_mint,
        pyth_spy,
        pyth_qqq,
        pyth_iwm,
        policy,
        policy_address,
    );
    vec![unwrap_ix, buyback_ix]
}
