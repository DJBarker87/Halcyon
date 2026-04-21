//! Halcyon operator CLI.
//!
//! Surface per `build_order_part4_layer2_plan.md` §3.6:
//!
//!   Admin bring-up        : init-protocol, register-sol-autocall, rotate-keeper
//!   Capital               : senior-deposit, seed-junior, fund-coupon-vault, fund-sleeve, defund-sleeve, sweep-fees
//!   Product (SOL Autocall): preview, buy, settle
//!   Product (IL Protect.) : preview-il, buy-il, settle-il, regime-status
//!   Keepers               : keepers fire-observation, keepers fire-regime
//!   Ops                   : status

mod client;
mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "halcyon", about = "Halcyon operator CLI", version)]
struct Cli {
    /// RPC endpoint. Defaults to localnet; override for devnet/mainnet.
    #[arg(long, default_value = "http://127.0.0.1:8899", global = true)]
    rpc: String,

    /// Admin/operator keypair file. Required for every mutating subcommand.
    #[arg(long, global = true)]
    keypair: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// One-shot admin bring-up: creates `ProtocolConfig` and the vault
    /// authority PDAs. Idempotent — skips if `ProtocolConfig` already exists.
    InitProtocol(commands::init_protocol::Args),

    /// Register the SOL Autocall product with the kernel. Admin-only.
    RegisterSolAutocall(commands::register::Args),

    /// Register the IL Protection product with the kernel. Admin-only.
    RegisterIlProtection(commands::register_il_protection::Args),

    /// Register the Flagship Autocall product with the kernel. Admin-only.
    RegisterFlagshipAutocall(commands::register_flagship_autocall::Args),

    /// Rotate a keeper role's authority. Admin-only.
    RotateKeeper(commands::rotate_keeper::Args),

    /// Resize/migrate the singleton ProtocolConfig account to the current layout.
    MigrateProtocolConfig(commands::migrate_protocol_config::Args),

    /// Update selected protocol-config fields. Admin-only.
    SetProtocolConfig(commands::set_protocol_config::Args),

    /// Senior-tranche deposit (user flow).
    SeniorDeposit(commands::senior_deposit::Args),

    /// Junior-tranche seeding. Admin-only at v1.
    SeedJunior(commands::seed_junior::Args),

    /// Top up the SOL Autocall coupon vault. Admin-only.
    FundCouponVault(commands::fund_coupon_vault::Args),

    /// Top up the SOL Autocall hedge sleeve custody. Admin-only.
    #[command(alias = "fund-hedge-sleeve")]
    FundSleeve(commands::fund_hedge_sleeve::Args),

    /// Withdraw USDC from the SOL Autocall hedge sleeve custody. Admin-only.
    #[command(alias = "defund-hedge-sleeve")]
    DefundSleeve(commands::defund_hedge_sleeve::Args),

    /// Sweep accrued treasury fees. Admin-only.
    SweepFees(commands::sweep_fees::Args),

    /// Permissionless: advance a product's `vault_sigma` EWMA from the given
    /// Pyth feed. First call stamps the initial price + timestamp so product
    /// `preview_quote` paths can pass the sigma-freshness gate on devnet.
    UpdateEwma(commands::update_ewma::Args),

    /// Simulate `preview_quote` and decode the Anchor return data.
    Preview(commands::preview::Args),

    /// Simulate IL Protection `preview_quote` and decode the Anchor return data.
    PreviewIl(commands::preview_il::Args),

    /// Simulate Flagship Autocall `preview_quote` and decode the Anchor return data.
    PreviewFlagship(commands::preview_flagship::Args),

    /// Issue a SOL Autocall policy.
    Buy(commands::buy::Args),

    /// Issue an IL Protection policy.
    BuyIl(commands::buy_il::Args),

    /// Trigger settlement on a matured or auto-called policy.
    Settle(commands::settle::Args),

    /// Trigger IL Protection settlement on a matured policy.
    SettleIl(commands::settle_il::Args),

    /// Show the current IL Protection regime signal.
    RegimeStatus,

    /// Keeper force-firing utilities (localnet / ops).
    Keepers {
        #[command(subcommand)]
        cmd: commands::keepers::KeeperCmd,
    },

    /// Dump active policies, VaultState, HedgeBookState, FeeLedger.
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let ctx = client::CliContext::new(&cli.rpc, cli.keypair.as_deref()).await?;

    match cli.command {
        Command::InitProtocol(a) => commands::init_protocol::run(&ctx, a).await,
        Command::RegisterSolAutocall(a) => commands::register::run(&ctx, a).await,
        Command::RegisterIlProtection(a) => commands::register_il_protection::run(&ctx, a).await,
        Command::RegisterFlagshipAutocall(a) => {
            commands::register_flagship_autocall::run(&ctx, a).await
        }
        Command::RotateKeeper(a) => commands::rotate_keeper::run(&ctx, a).await,
        Command::MigrateProtocolConfig(a) => commands::migrate_protocol_config::run(&ctx, a).await,
        Command::SetProtocolConfig(a) => commands::set_protocol_config::run(&ctx, a).await,
        Command::SeniorDeposit(a) => commands::senior_deposit::run(&ctx, a).await,
        Command::SeedJunior(a) => commands::seed_junior::run(&ctx, a).await,
        Command::FundCouponVault(a) => commands::fund_coupon_vault::run(&ctx, a).await,
        Command::FundSleeve(a) => commands::fund_hedge_sleeve::run(&ctx, a).await,
        Command::DefundSleeve(a) => commands::defund_hedge_sleeve::run(&ctx, a).await,
        Command::SweepFees(a) => commands::sweep_fees::run(&ctx, a).await,
        Command::UpdateEwma(a) => commands::update_ewma::run(&ctx, a).await,
        Command::Preview(a) => commands::preview::run(&ctx, a).await,
        Command::PreviewIl(a) => commands::preview_il::run(&ctx, a).await,
        Command::PreviewFlagship(a) => commands::preview_flagship::run(&ctx, a).await,
        Command::Buy(a) => commands::buy::run(&ctx, a).await,
        Command::BuyIl(a) => commands::buy_il::run(&ctx, a).await,
        Command::Settle(a) => commands::settle::run(&ctx, a).await,
        Command::SettleIl(a) => commands::settle_il::run(&ctx, a).await,
        Command::RegimeStatus => commands::regime_status::run(&ctx).await,
        Command::Keepers { cmd } => commands::keepers::run(&ctx, cmd).await,
        Command::Status => commands::status::run(&ctx).await,
    }
}
