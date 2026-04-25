use anyhow::Result;
use clap::Args as ClapArgs;
use solana_sdk::signer::Signer;

use halcyon_client_sdk::{kernel, pda, tx};

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
    let ix = kernel::fund_hedge_sleeve_ix(
        &admin.pubkey(),
        &usdc_mint,
        &halcyon_sol_autocall::ID,
        args.amount,
    );
    let signature = tx::send_instructions(ctx.rpc.as_ref(), admin, vec![ix]).await?;
    println!(
        "fund-sleeve: signature={signature} amount={} product_program_id={} hedge_sleeve={} hedge_sleeve_usdc={}",
        args.amount,
        halcyon_sol_autocall::ID,
        pda::hedge_sleeve(&halcyon_sol_autocall::ID).0,
        pda::hedge_sleeve_usdc(&halcyon_sol_autocall::ID, &usdc_mint)
    );
    Ok(())
}
