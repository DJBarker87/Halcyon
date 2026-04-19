use anyhow::Result;
use clap::Args as ClapArgs;
use solana_sdk::signature::Keypair;

use halcyon_client_sdk::sol_autocall;

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    pub notional: u64,
    #[arg(long)]
    pub pyth_sol: String,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let pyth_sol = CliContext::parse_pubkey("pyth_sol", &args.pyth_sol)?;
    let ephemeral = Keypair::new();
    let payer = ctx.signer.as_ref().unwrap_or(&ephemeral);
    let quote =
        sol_autocall::simulate_preview_quote(ctx.rpc.as_ref(), payer, pyth_sol, args.notional)
            .await?;
    println!("preview:");
    println!("  notional_usdc={}", args.notional);
    let no_quote = quote.max_liability == 0
        || quote.fair_coupon_bps_s6 == 0
        || quote.offered_coupon_bps_s6 == 0;
    println!("  no_quote={no_quote}");
    if no_quote {
        println!("  reason=fair coupon below 50 bps issuance floor or pricing confidence is low");
        println!("  entry_price_s6={}", quote.entry_price_s6);
        println!("  engine_version={}", quote.engine_version);
        return Ok(());
    }

    let liability_buffer = quote.max_liability.saturating_sub(args.notional);
    println!("  principal_escrow_usdc={}", args.notional);
    println!("  premium_usdc={}", quote.premium);
    println!("  max_liability_usdc={}", quote.max_liability);
    println!("  liability_buffer_usdc={liability_buffer}");
    println!("  fair_coupon_bps_s6={}", quote.fair_coupon_bps_s6);
    println!("  offered_coupon_bps_s6={}", quote.offered_coupon_bps_s6);
    println!("  entry_price_s6={}", quote.entry_price_s6);
    println!("  expiry_ts={}", quote.expiry_ts);
    println!("  quote_slot={}", quote.quote_slot);
    println!("  engine_version={}", quote.engine_version);
    Ok(())
}
