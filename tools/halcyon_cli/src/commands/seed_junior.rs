use anyhow::Result;
use clap::Args as ClapArgs;
use solana_sdk::signer::Signer;

use halcyon_client_sdk::{kernel, tx};

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    pub amount: u64,
    #[arg(long)]
    pub usdc_mint: String,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let admin = ctx.signer()?;
    let usdc_mint = CliContext::parse_pubkey("usdc_mint", &args.usdc_mint)?;
    let ix = kernel::seed_junior_ix(&admin.pubkey(), &usdc_mint, args.amount);
    let signature = tx::send_instructions(ctx.rpc.as_ref(), admin, vec![ix]).await?;
    println!(
        "seed-junior: signature={signature} amount={} usdc_mint={usdc_mint}",
        args.amount
    );
    Ok(())
}
