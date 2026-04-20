use anchor_lang::{prelude::Pubkey, Discriminator, InstructionData, ToAccountMetas};
use solana_sdk::{instruction::Instruction, system_program};

use crate::pda;

pub use halcyon_kernel::{
    ApplySettlementArgs, InitializeProtocolArgs, PrepareHedgeSwapArgs, RecordHedgeTradeArgs,
    RegisterProductArgs, SettlementReason, WriteAggregateDeltaArgs, WriteRegimeSignalArgs,
    WriteRegressionArgs,
};

pub fn initialize_protocol_ix(
    admin: &Pubkey,
    usdc_mint: &Pubkey,
    args: InitializeProtocolArgs,
) -> Instruction {
    let (protocol_config, _) = pda::protocol_config();
    let (vault_state, _) = pda::vault_state();
    let (fee_ledger, _) = pda::fee_ledger();
    let (keeper_registry, _) = pda::keeper_registry();
    let (vault_authority, _) = pda::vault_authority();
    let (vault_usdc, _) = pda::vault_usdc(usdc_mint);
    let (treasury_usdc, _) = pda::treasury_usdc(usdc_mint);
    Instruction {
        program_id: halcyon_kernel::ID,
        accounts: halcyon_kernel::accounts::InitializeProtocol {
            admin: *admin,
            protocol_config,
            vault_state,
            fee_ledger,
            keeper_registry,
            usdc_mint: *usdc_mint,
            vault_authority,
            vault_usdc,
            treasury_usdc,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
            rent: solana_sdk::sysvar::rent::ID,
        }
        .to_account_metas(None),
        data: halcyon_kernel::instruction::InitializeProtocol { args }.data(),
    }
}

pub fn register_sol_autocall_ix(
    admin: &Pubkey,
    per_policy_risk_cap: u64,
    global_risk_cap: u64,
) -> Instruction {
    use halcyon_sol_autocall::state::SolAutocallTerms;

    let (protocol_config, _) = pda::protocol_config();
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_sol_autocall::ID);
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_sol_autocall::ID);
    let (product_authority, _) = pda::product_authority();
    let mut init_terms_discriminator = [0u8; 8];
    init_terms_discriminator.copy_from_slice(SolAutocallTerms::DISCRIMINATOR);
    let args = RegisterProductArgs {
        product_program_id: halcyon_sol_autocall::ID,
        expected_authority: product_authority,
        oracle_feed_id: halcyon_oracles::feed_ids::SOL_USD,
        per_policy_risk_cap,
        global_risk_cap,
        engine_version: halcyon_sol_autocall::state::CURRENT_ENGINE_VERSION,
        init_terms_discriminator,
        // SOL Autocall is principal-backed: buyer escrows notional on issuance.
        requires_principal_escrow: true,
    };
    Instruction {
        program_id: halcyon_kernel::ID,
        accounts: halcyon_kernel::accounts::RegisterProduct {
            admin: *admin,
            protocol_config,
            product_registry_entry,
            vault_sigma,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: halcyon_kernel::instruction::RegisterProduct { args }.data(),
    }
}

pub fn register_il_protection_ix(
    admin: &Pubkey,
    per_policy_risk_cap: u64,
    global_risk_cap: u64,
) -> Instruction {
    use anchor_lang::Discriminator;
    use halcyon_il_protection::state::IlProtectionTerms;

    let (protocol_config, _) = pda::protocol_config();
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_il_protection::ID);
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_il_protection::ID);
    let (product_authority, _) = pda::product_authority_for(&halcyon_il_protection::ID);
    let mut init_terms_discriminator = [0u8; 8];
    init_terms_discriminator.copy_from_slice(IlProtectionTerms::DISCRIMINATOR);
    let args = RegisterProductArgs {
        product_program_id: halcyon_il_protection::ID,
        expected_authority: product_authority,
        oracle_feed_id: halcyon_oracles::feed_ids::SOL_USD,
        per_policy_risk_cap,
        global_risk_cap,
        engine_version: halcyon_il_protection::state::CURRENT_ENGINE_VERSION,
        init_terms_discriminator,
        // IL Protection is synthetic: buyer pays premium only; coverage
        // comes from senior+junior tranche capital, not buyer principal.
        requires_principal_escrow: false,
    };
    Instruction {
        program_id: halcyon_kernel::ID,
        accounts: halcyon_kernel::accounts::RegisterProduct {
            admin: *admin,
            protocol_config,
            product_registry_entry,
            vault_sigma,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: halcyon_kernel::instruction::RegisterProduct { args }.data(),
    }
}

pub fn register_flagship_autocall_ix(
    admin: &Pubkey,
    per_policy_risk_cap: u64,
    global_risk_cap: u64,
) -> Instruction {
    use anchor_lang::Discriminator;
    use halcyon_flagship_autocall::state::FlagshipAutocallTerms;

    let (protocol_config, _) = pda::protocol_config();
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_flagship_autocall::ID);
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_flagship_autocall::ID);
    let (product_authority, _) = pda::product_authority_for(&halcyon_flagship_autocall::ID);
    let mut init_terms_discriminator = [0u8; 8];
    init_terms_discriminator.copy_from_slice(FlagshipAutocallTerms::DISCRIMINATOR);
    let args = RegisterProductArgs {
        product_program_id: halcyon_flagship_autocall::ID,
        expected_authority: product_authority,
        oracle_feed_id: halcyon_oracles::feed_ids::SPY_USD,
        per_policy_risk_cap,
        global_risk_cap,
        engine_version: halcyon_flagship_autocall::state::CURRENT_ENGINE_VERSION,
        init_terms_discriminator,
        // Flagship autocall is principal-backed: buyer escrows notional.
        requires_principal_escrow: true,
    };
    Instruction {
        program_id: halcyon_kernel::ID,
        accounts: halcyon_kernel::accounts::RegisterProduct {
            admin: *admin,
            protocol_config,
            product_registry_entry,
            vault_sigma,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: halcyon_kernel::instruction::RegisterProduct { args }.data(),
    }
}

pub fn rotate_keeper_ix(admin: &Pubkey, role: u8, new_authority: Pubkey) -> Instruction {
    let (protocol_config, _) = pda::protocol_config();
    let (keeper_registry, _) = pda::keeper_registry();
    Instruction {
        program_id: halcyon_kernel::ID,
        accounts: halcyon_kernel::accounts::RotateKeeper {
            admin: *admin,
            protocol_config,
            keeper_registry,
        }
        .to_account_metas(None),
        data: halcyon_kernel::instruction::RotateKeeper {
            role,
            new_authority,
        }
        .data(),
    }
}

pub fn write_regime_signal_ix(
    keeper: &Pubkey,
    payer: &Pubkey,
    product_program_id: &Pubkey,
    args: WriteRegimeSignalArgs,
) -> Instruction {
    let (protocol_config, _) = pda::protocol_config();
    let (keeper_registry, _) = pda::keeper_registry();
    let (regime_signal, _) = pda::regime_signal(product_program_id);
    Instruction {
        program_id: halcyon_kernel::ID,
        accounts: halcyon_kernel::accounts::WriteRegimeSignal {
            keeper: *keeper,
            protocol_config,
            keeper_registry,
            regime_signal,
            payer: *payer,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: halcyon_kernel::instruction::WriteRegimeSignal { args }.data(),
    }
}

pub fn write_regression_ix(
    keeper: &Pubkey,
    payer: &Pubkey,
    args: WriteRegressionArgs,
) -> Instruction {
    let (protocol_config, _) = pda::protocol_config();
    let (keeper_registry, _) = pda::keeper_registry();
    let (regression, _) = pda::regression();
    Instruction {
        program_id: halcyon_kernel::ID,
        accounts: halcyon_kernel::accounts::WriteRegression {
            keeper: *keeper,
            protocol_config,
            keeper_registry,
            regression,
            payer: *payer,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: halcyon_kernel::instruction::WriteRegression { args }.data(),
    }
}

pub fn write_aggregate_delta_ix(
    keeper: &Pubkey,
    payer: &Pubkey,
    args: WriteAggregateDeltaArgs,
) -> Instruction {
    let (keeper_registry, _) = pda::keeper_registry();
    let (product_registry_entry, _) = pda::product_registry_entry(&args.product_program_id);
    let (aggregate_delta, _) = pda::aggregate_delta(&args.product_program_id);
    Instruction {
        program_id: halcyon_kernel::ID,
        accounts: halcyon_kernel::accounts::WriteAggregateDelta {
            keeper: *keeper,
            keeper_registry,
            product_registry_entry,
            aggregate_delta,
            payer: *payer,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: halcyon_kernel::instruction::WriteAggregateDelta { args }.data(),
    }
}

pub fn deposit_senior_ix(depositor: &Pubkey, usdc_mint: &Pubkey, amount: u64) -> Instruction {
    let depositor_usdc = pda::associated_token_account(depositor, usdc_mint);
    let (vault_usdc, _) = pda::vault_usdc(usdc_mint);
    let (protocol_config, _) = pda::protocol_config();
    let (vault_state, _) = pda::vault_state();
    let (senior_deposit, _) = pda::senior(depositor);
    Instruction {
        program_id: halcyon_kernel::ID,
        accounts: halcyon_kernel::accounts::DepositSenior {
            depositor: *depositor,
            usdc_mint: *usdc_mint,
            depositor_usdc,
            vault_usdc,
            protocol_config,
            vault_state,
            senior_deposit,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: halcyon_kernel::instruction::DepositSenior { amount }.data(),
    }
}

pub fn seed_junior_ix(admin: &Pubkey, usdc_mint: &Pubkey, amount: u64) -> Instruction {
    let (protocol_config, _) = pda::protocol_config();
    let (vault_state, _) = pda::vault_state();
    let admin_usdc = pda::associated_token_account(admin, usdc_mint);
    let (vault_usdc, _) = pda::vault_usdc(usdc_mint);
    let (junior, _) = pda::junior(admin);
    Instruction {
        program_id: halcyon_kernel::ID,
        accounts: halcyon_kernel::accounts::SeedJunior {
            admin: *admin,
            usdc_mint: *usdc_mint,
            protocol_config,
            vault_state,
            admin_usdc,
            vault_usdc,
            junior,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: halcyon_kernel::instruction::SeedJunior { amount }.data(),
    }
}

pub fn fund_coupon_vault_ix(
    admin: &Pubkey,
    usdc_mint: &Pubkey,
    product_program_id: &Pubkey,
    amount: u64,
) -> Instruction {
    let (protocol_config, _) = pda::protocol_config();
    let (product_registry_entry, _) = pda::product_registry_entry(product_program_id);
    let admin_usdc = pda::associated_token_account(admin, usdc_mint);
    let (coupon_vault, _) = pda::coupon_vault(product_program_id);
    let coupon_vault_usdc = pda::coupon_vault_usdc(product_program_id, usdc_mint);
    Instruction {
        program_id: halcyon_kernel::ID,
        accounts: halcyon_kernel::accounts::FundCouponVault {
            admin: *admin,
            usdc_mint: *usdc_mint,
            protocol_config,
            product_registry_entry,
            admin_usdc,
            coupon_vault,
            coupon_vault_usdc,
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: halcyon_kernel::instruction::FundCouponVault {
            product_program_id: *product_program_id,
            amount,
        }
        .data(),
    }
}

pub fn fund_hedge_sleeve_ix(
    admin: &Pubkey,
    usdc_mint: &Pubkey,
    product_program_id: &Pubkey,
    amount: u64,
) -> Instruction {
    let (protocol_config, _) = pda::protocol_config();
    let (product_registry_entry, _) = pda::product_registry_entry(product_program_id);
    let (hedge_sleeve, _) = pda::hedge_sleeve(product_program_id);
    let admin_usdc = pda::associated_token_account(admin, usdc_mint);
    let hedge_sleeve_usdc = pda::hedge_sleeve_usdc(product_program_id, usdc_mint);
    Instruction {
        program_id: halcyon_kernel::ID,
        accounts: halcyon_kernel::accounts::FundHedgeSleeve {
            admin: *admin,
            usdc_mint: *usdc_mint,
            protocol_config,
            product_registry_entry,
            hedge_sleeve,
            admin_usdc,
            hedge_sleeve_usdc,
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: halcyon_kernel::instruction::FundHedgeSleeve {
            product_program_id: *product_program_id,
            amount,
        }
        .data(),
    }
}

pub fn defund_hedge_sleeve_ix(
    admin: &Pubkey,
    usdc_mint: &Pubkey,
    product_program_id: &Pubkey,
    destination_usdc: &Pubkey,
    amount: u64,
) -> Instruction {
    let (protocol_config, _) = pda::protocol_config();
    let (product_registry_entry, _) = pda::product_registry_entry(product_program_id);
    let (hedge_sleeve, _) = pda::hedge_sleeve(product_program_id);
    let hedge_sleeve_usdc = pda::hedge_sleeve_usdc(product_program_id, usdc_mint);
    Instruction {
        program_id: halcyon_kernel::ID,
        accounts: halcyon_kernel::accounts::DefundHedgeSleeve {
            admin: *admin,
            usdc_mint: *usdc_mint,
            protocol_config,
            product_registry_entry,
            hedge_sleeve,
            hedge_sleeve_usdc,
            destination_usdc: *destination_usdc,
            token_program: anchor_spl::token::ID,
        }
        .to_account_metas(None),
        data: halcyon_kernel::instruction::DefundHedgeSleeve {
            product_program_id: *product_program_id,
            amount,
        }
        .data(),
    }
}

pub fn sweep_fees_ix(
    admin: &Pubkey,
    usdc_mint: &Pubkey,
    destination: &Pubkey,
    amount: u64,
) -> Instruction {
    let (protocol_config, _) = pda::protocol_config();
    let (fee_ledger, _) = pda::fee_ledger();
    let (treasury_usdc, _) = pda::treasury_usdc(usdc_mint);
    let (vault_authority, _) = pda::vault_authority();
    Instruction {
        program_id: halcyon_kernel::ID,
        accounts: halcyon_kernel::accounts::SweepFees {
            admin: *admin,
            usdc_mint: *usdc_mint,
            protocol_config,
            fee_ledger,
            treasury_usdc,
            vault_authority,
            destination_usdc: *destination,
            token_program: anchor_spl::token::ID,
        }
        .to_account_metas(None),
        data: halcyon_kernel::instruction::SweepFees { amount }.data(),
    }
}

pub fn record_hedge_trade_ix(
    keeper: &Pubkey,
    usdc_mint: &Pubkey,
    args: RecordHedgeTradeArgs,
) -> Instruction {
    let (keeper_registry, _) = pda::keeper_registry();
    let (product_registry_entry, _) = pda::product_registry_entry(&args.product_program_id);
    let (hedge_book, _) = pda::hedge_book(&args.product_program_id);
    let (hedge_sleeve, _) = pda::hedge_sleeve(&args.product_program_id);
    let (pending_hedge_swap, _) = pda::pending_hedge_swap(&args.product_program_id);
    let hedge_sleeve_usdc = pda::hedge_sleeve_usdc(&args.product_program_id, usdc_mint);
    let hedge_sleeve_wsol = pda::hedge_sleeve_wsol(&args.product_program_id);
    Instruction {
        program_id: halcyon_kernel::ID,
        accounts: halcyon_kernel::accounts::RecordHedgeTrade {
            keeper: *keeper,
            keeper_registry,
            product_registry_entry,
            hedge_book,
            hedge_sleeve,
            pending_hedge_swap,
            hedge_sleeve_usdc,
            usdc_mint: *usdc_mint,
            hedge_sleeve_wsol,
            token_program: anchor_spl::token::ID,
        }
        .to_account_metas(None),
        data: halcyon_kernel::instruction::RecordHedgeTrade { args }.data(),
    }
}

pub fn prepare_hedge_swap_ix(
    keeper: &Pubkey,
    payer: &Pubkey,
    usdc_mint: &Pubkey,
    pyth_sol: &Pubkey,
    args: PrepareHedgeSwapArgs,
) -> Instruction {
    let (keeper_registry, _) = pda::keeper_registry();
    let (product_registry_entry, _) = pda::product_registry_entry(&args.product_program_id);
    let (protocol_config, _) = pda::protocol_config();
    let (hedge_book, _) = pda::hedge_book(&args.product_program_id);
    let (hedge_sleeve, _) = pda::hedge_sleeve(&args.product_program_id);
    let (pending_hedge_swap, _) = pda::pending_hedge_swap(&args.product_program_id);
    let hedge_sleeve_usdc = pda::hedge_sleeve_usdc(&args.product_program_id, usdc_mint);
    let hedge_sleeve_wsol = pda::hedge_sleeve_wsol(&args.product_program_id);

    Instruction {
        program_id: halcyon_kernel::ID,
        accounts: halcyon_kernel::accounts::PrepareHedgeSwap {
            keeper: *keeper,
            payer: *payer,
            keeper_registry,
            product_registry_entry,
            protocol_config,
            hedge_book,
            hedge_sleeve,
            pending_hedge_swap,
            pyth_sol: *pyth_sol,
            usdc_mint: *usdc_mint,
            wsol_mint: anchor_spl::token::spl_token::native_mint::ID,
            hedge_sleeve_usdc,
            hedge_sleeve_wsol,
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
            instructions: solana_sdk::sysvar::instructions::ID,
        }
        .to_account_metas(None),
        data: halcyon_kernel::instruction::PrepareHedgeSwap { args }.data(),
    }
}
