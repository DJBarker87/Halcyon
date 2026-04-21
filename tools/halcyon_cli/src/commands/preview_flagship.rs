use anyhow::Result;
use clap::Args as ClapArgs;
use solana_sdk::signature::Keypair;

use halcyon_client_sdk::flagship_autocall;

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
    let quote = flagship_autocall::simulate_preview_quote(
        ctx.rpc.as_ref(),
        payer,
        pyth_spy,
        pyth_qqq,
        pyth_iwm,
        args.notional,
    )
    .await?;
    println!("preview-flagship:");
    println!("  notional_usdc={}", args.notional);
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
