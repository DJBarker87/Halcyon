use anyhow::Result;
use clap::Args as ClapArgs;
use halcyon_client_sdk::{kernel::update_ewma_ix, tx::send_instructions};

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Product alias whose `vault_sigma` should be updated: `sol`, `il`, or `flagship`.
    #[arg(long, default_value = "sol")]
    pub product: String,
    /// Pyth `PriceUpdateV2` account for the feed matching the product's
    /// `vault_sigma.oracle_feed_id`.
    #[arg(long)]
    pub oracle_price: String,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let payer = ctx.signer()?;
    let oracle_price = CliContext::parse_pubkey("oracle_price", &args.oracle_price)?;
    let product_program_id = match args.product.to_ascii_lowercase().as_str() {
        "sol" | "sol_autocall" | "sol-autocall" => halcyon_sol_autocall::ID,
        "il" | "il_protection" | "il-protection" => halcyon_il_protection::ID,
        "flagship" | "flagship_autocall" | "flagship-autocall" => halcyon_flagship_autocall::ID,
        other => anyhow::bail!("unknown --product '{other}' (expected: sol, il, flagship)"),
    };
    let ix = update_ewma_ix(&product_program_id, &oracle_price);
    let sig = send_instructions(&ctx.rpc, payer, vec![ix]).await?;
    println!("update-ewma: sig={sig} product={product_program_id} oracle={oracle_price}");
    Ok(())
}
