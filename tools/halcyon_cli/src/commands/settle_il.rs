use anyhow::Result;
use clap::Args as ClapArgs;
use solana_sdk::signer::Signer;

use halcyon_client_sdk::{decode::fetch_anchor_account, il_protection, tx};

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    pub policy: String,
    #[arg(long, value_name = "PUBKEY")]
    pub usdc_mint: Option<String>,
    #[arg(long)]
    pub pyth_sol: String,
    #[arg(long)]
    pub pyth_usdc: String,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let caller = ctx.signer()?;
    let policy = CliContext::parse_pubkey("policy", &args.policy)?;
    let usdc_mint = ctx.resolve_usdc_mint(args.usdc_mint.as_deref())?;
    let pyth_sol = CliContext::parse_pubkey("pyth_sol", &args.pyth_sol)?;
    let pyth_usdc = CliContext::parse_pubkey("pyth_usdc", &args.pyth_usdc)?;
    let header =
        fetch_anchor_account::<halcyon_kernel::state::PolicyHeader>(ctx.rpc.as_ref(), &policy)
            .await?;
    let ix = il_protection::settle_ix(
        &caller.pubkey(),
        &usdc_mint,
        pyth_sol,
        pyth_usdc,
        &header,
        policy,
    );
    let signature = tx::send_instructions(ctx.rpc.as_ref(), caller, vec![ix]).await?;
    println!("settle-il: signature={signature} policy={policy}");
    Ok(())
}
