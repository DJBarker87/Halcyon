//! Halcyon kernel — money, reserves, tranches, policy lifecycle.
//!
//! Every kernel-owned PDA has its layout documented in `LAYOUTS.md` alongside
//! this file. `make layouts-check` verifies parity against the compiled IDL.

use anchor_lang::prelude::*;

pub mod errors;
pub mod instructions;
pub mod state;

pub use errors::KernelError;
pub use instructions::*;
pub use state::*;

declare_id!("H71FxCTuVGL13PkzXeVxeTn89xZreFm4AwLu3iZeVtdF");

#[program]
pub mod halcyon_kernel {
    use super::*;

    // --- Admin ---

    pub fn initialize_protocol(
        ctx: Context<InitializeProtocol>,
        args: InitializeProtocolArgs,
    ) -> Result<()> {
        instructions::admin::initialize_protocol::handler(ctx, args)
    }

    pub fn set_protocol_config(
        ctx: Context<SetProtocolConfig>,
        args: SetProtocolConfigArgs,
    ) -> Result<()> {
        instructions::admin::set_protocol_config::handler(ctx, args)
    }

    pub fn migrate_protocol_config(ctx: Context<MigrateProtocolConfig>) -> Result<()> {
        instructions::admin::migrate_protocol_config::handler(ctx)
    }

    pub fn pause_issuance(ctx: Context<SetPauseFlag>, paused: bool) -> Result<()> {
        instructions::admin::pause_issuance::handler(ctx, paused)
    }

    pub fn pause_settlement(ctx: Context<SetPauseFlag>, paused: bool) -> Result<()> {
        instructions::admin::pause_settlement::handler(ctx, paused)
    }

    pub fn rotate_keeper(
        ctx: Context<RotateKeeper>,
        role: u8,
        new_authority: Pubkey,
    ) -> Result<()> {
        instructions::admin::rotate_keeper::handler(ctx, role, new_authority)
    }

    pub fn register_product(
        ctx: Context<RegisterProduct>,
        args: RegisterProductArgs,
    ) -> Result<()> {
        instructions::admin::register_product::handler(ctx, args)
    }

    pub fn update_product_registry(
        ctx: Context<UpdateProductRegistry>,
        args: UpdateProductRegistryArgs,
    ) -> Result<()> {
        instructions::admin::update_product_registry::handler(ctx, args)
    }

    pub fn register_lookup_table(
        ctx: Context<RegisterLookupTable>,
        lookup_table: Pubkey,
    ) -> Result<()> {
        instructions::admin::register_lookup_table::handler(ctx, lookup_table)
    }

    pub fn update_lookup_table(
        ctx: Context<UpdateLookupTable>,
        index: u8,
        new_lookup_table: Pubkey,
    ) -> Result<()> {
        instructions::admin::update_lookup_table::handler(ctx, index, new_lookup_table)
    }

    // --- Capital ---

    pub fn deposit_senior(ctx: Context<DepositSenior>, amount: u64) -> Result<()> {
        instructions::capital::deposit_senior::handler(ctx, amount)
    }

    pub fn withdraw_senior(ctx: Context<WithdrawSenior>, amount: u64) -> Result<()> {
        instructions::capital::withdraw_senior::handler(ctx, amount)
    }

    pub fn seed_junior(ctx: Context<SeedJunior>, amount: u64) -> Result<()> {
        instructions::capital::seed_junior::handler(ctx, amount)
    }

    pub fn sweep_fees(ctx: Context<SweepFees>, amount: u64) -> Result<()> {
        instructions::capital::sweep_fees::handler(ctx, amount)
    }

    pub fn fund_coupon_vault(
        ctx: Context<FundCouponVault>,
        product_program_id: Pubkey,
        amount: u64,
    ) -> Result<()> {
        instructions::capital::fund_coupon_vault::handler(ctx, product_program_id, amount)
    }

    pub fn fund_hedge_sleeve(
        ctx: Context<FundHedgeSleeve>,
        product_program_id: Pubkey,
        amount: u64,
    ) -> Result<()> {
        instructions::capital::fund_hedge_sleeve::handler(ctx, product_program_id, amount)
    }

    pub fn defund_hedge_sleeve(
        ctx: Context<DefundHedgeSleeve>,
        product_program_id: Pubkey,
        amount: u64,
    ) -> Result<()> {
        instructions::capital::defund_hedge_sleeve::handler(ctx, product_program_id, amount)
    }

    // --- Oracle writes ---

    pub fn update_ewma(ctx: Context<UpdateEwma>) -> Result<()> {
        instructions::oracle::update_ewma::handler(ctx)
    }

    pub fn write_regression(
        ctx: Context<WriteRegression>,
        args: WriteRegressionArgs,
    ) -> Result<()> {
        instructions::oracle::write_regression::handler(ctx, args)
    }

    pub fn write_regime_signal(
        ctx: Context<WriteRegimeSignal>,
        args: WriteRegimeSignalArgs,
    ) -> Result<()> {
        instructions::oracle::write_regime_signal::handler(ctx, args)
    }

    pub fn write_aggregate_delta(
        ctx: Context<WriteAggregateDelta>,
        args: WriteAggregateDeltaArgs,
    ) -> Result<()> {
        instructions::oracle::write_aggregate_delta::handler(ctx, args)
    }

    // --- Policy lifecycle ---

    pub fn reserve_and_issue(
        ctx: Context<ReserveAndIssue>,
        args: ReserveAndIssueArgs,
    ) -> Result<()> {
        instructions::lifecycle::reserve_and_issue::handler(ctx, args)
    }

    pub fn finalize_policy(ctx: Context<FinalizePolicy>) -> Result<()> {
        instructions::lifecycle::finalize_policy::handler(ctx)
    }

    pub fn apply_settlement(
        ctx: Context<ApplySettlement>,
        args: ApplySettlementArgs,
    ) -> Result<()> {
        instructions::lifecycle::apply_settlement::handler(ctx, args)
    }

    pub fn pay_coupon(ctx: Context<PayCoupon>, args: PayCouponArgs) -> Result<()> {
        instructions::lifecycle::pay_coupon::handler(ctx, args)
    }

    pub fn reap_quoted(ctx: Context<ReapQuoted>) -> Result<()> {
        instructions::lifecycle::reap_quoted::handler(ctx)
    }

    pub fn record_hedge_trade(
        ctx: Context<RecordHedgeTrade>,
        args: RecordHedgeTradeArgs,
    ) -> Result<()> {
        instructions::lifecycle::record_hedge_trade::handler(ctx, args)
    }

    pub fn prepare_hedge_swap(
        ctx: Context<PrepareHedgeSwap>,
        args: PrepareHedgeSwapArgs,
    ) -> Result<()> {
        instructions::lifecycle::prepare_hedge_swap::handler(ctx, args)
    }
}
