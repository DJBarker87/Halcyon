use anyhow::Result;
use clap::Args as ClapArgs;
use solana_sdk::signature::Keypair;

use halcyon_client_sdk::{flagship_autocall, pda, tx};

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    pub notional: u64,
    #[arg(long)]
    pub pyth_spy: String,
    #[arg(long)]
    pub pyth_qqq: String,
    #[arg(long)]
    pub pyth_iwm: String,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let pyth_spy = CliContext::parse_pubkey("pyth_spy", &args.pyth_spy)?;
    let pyth_qqq = CliContext::parse_pubkey("pyth_qqq", &args.pyth_qqq)?;
    let pyth_iwm = CliContext::parse_pubkey("pyth_iwm", &args.pyth_iwm)?;
    let ephemeral = Keypair::new();
    let payer = ctx.signer.as_ref().unwrap_or(&ephemeral);
    let (protocol_config, _) = pda::protocol_config();
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_flagship_autocall::ID);
    let (vault_sigma, _) = pda::vault_sigma(&halcyon_flagship_autocall::ID);
    let (regression, _) = pda::regression();
    let (autocall_schedule, _) = pda::autocall_schedule(&halcyon_flagship_autocall::ID);
    let ix = flagship_autocall::preview_quote_ix(
        protocol_config,
        product_registry_entry,
        vault_sigma,
        regression,
        autocall_schedule,
        pyth_spy,
        pyth_qqq,
        pyth_iwm,
        args.notional,
    );
    let result = tx::simulate_instruction(ctx.rpc.as_ref(), payer, ix).await?;
    if std::env::var_os("HALCYON_PRINT_SIM_LOGS").is_some() {
        if let Some(logs) = result.logs.as_ref() {
            for line in logs {
                println!("{line}");
            }
        }
    }
    let units_consumed = result.units_consumed;
    let quote: flagship_autocall::QuotePreview =
        tx::decode_return_data(result, &halcyon_flagship_autocall::ID)?;
    println!("preview-flagship:");
    println!("  notional_usdc={}", args.notional);
    if let Some(units) = units_consumed {
        println!("  compute_units_consumed={units}");
    } else {
        println!("  compute_units_consumed=<unavailable>");
    }
    println!("  premium={}", quote.premium);
    println!("  max_liability={}", quote.max_liability);
    println!("  fair_coupon_bps_s6={}", quote.fair_coupon_bps_s6);
    println!("  offered_coupon_bps_s6={}", quote.offered_coupon_bps_s6);
    println!("  sigma_pricing_s6={}", quote.sigma_pricing_s6);
    println!("  entry_spy_price_s6={}", quote.entry_spy_price_s6);
    println!("  entry_qqq_price_s6={}", quote.entry_qqq_price_s6);
    println!("  entry_iwm_price_s6={}", quote.entry_iwm_price_s6);
    println!("  quote_slot={}", quote.quote_slot);
    println!("  engine_version={}", quote.engine_version);
    let _ = quote.expiry_ts;
    Ok(())
}
