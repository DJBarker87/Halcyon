use anyhow::Result;
use clap::Args as ClapArgs;
use solana_sdk::signature::Keypair;

use halcyon_client_sdk::il_protection;

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    pub insured_notional: u64,
    #[arg(long)]
    pub pyth_sol: String,
    #[arg(long)]
    pub pyth_usdc: String,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let pyth_sol = CliContext::parse_pubkey("pyth_sol", &args.pyth_sol)?;
    let pyth_usdc = CliContext::parse_pubkey("pyth_usdc", &args.pyth_usdc)?;
    let ephemeral = Keypair::new();
    let payer = ctx.signer.as_ref().unwrap_or(&ephemeral);
    let quote = il_protection::simulate_preview_quote(
        ctx.rpc.as_ref(),
        payer,
        pyth_sol,
        pyth_usdc,
        args.insured_notional,
    )
    .await?;
    println!("preview-il:");
    println!("  insured_notional_usdc={}", args.insured_notional);
    println!("  premium_usdc={}", quote.premium);
    println!("  max_liability_usdc={}", quote.max_liability);
    println!(
        "  fair_premium_fraction_s6={}",
        quote.fair_premium_fraction_s6
    );
    println!(
        "  loaded_premium_fraction_s6={}",
        quote.loaded_premium_fraction_s6
    );
    println!("  sigma_pricing_s6={}", quote.sigma_pricing_s6);
    println!("  fvol_s6={}", quote.fvol_s6);
    println!("  regime={}", quote.regime);
    println!("  sigma_multiplier_s6={}", quote.sigma_multiplier_s6);
    println!("  entry_sol_price_s6={}", quote.entry_sol_price_s6);
    println!("  entry_usdc_price_s6={}", quote.entry_usdc_price_s6);
    println!("  expiry_ts={}", quote.expiry_ts);
    println!("  quote_slot={}", quote.quote_slot);
    println!("  engine_version={}", quote.engine_version);
    Ok(())
}
