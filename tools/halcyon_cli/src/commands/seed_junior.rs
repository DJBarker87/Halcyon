use anyhow::Result;
use clap::Args as ClapArgs;
use solana_sdk::signer::Signer;

use halcyon_client_sdk::{kernel, tx};

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    pub amount: u64,
    #[arg(long, value_name = "PUBKEY")]
    pub usdc_mint: Option<String>,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let admin = ctx.signer()?;
    let usdc_mint = ctx.resolve_usdc_mint(args.usdc_mint.as_deref())?;
    let ix = kernel::seed_junior_ix(&admin.pubkey(), &usdc_mint, args.amount);
    let signature = tx::send_instructions(ctx.rpc.as_ref(), admin, vec![ix]).await?;
    println!(
        "seed-junior: signature={signature} amount={} usdc_mint={usdc_mint}",
        args.amount
    );
    Ok(())
}
