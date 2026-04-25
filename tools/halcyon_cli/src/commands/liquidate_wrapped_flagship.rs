use anyhow::Result;
use clap::Args as ClapArgs;
use solana_sdk::signer::Signer;

use halcyon_client_sdk::{decode::fetch_anchor_account, flagship_autocall, tx};

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    pub policy: String,
    #[arg(long, value_name = "PUBKEY")]
    pub usdc_mint: Option<String>,
    #[arg(long)]
    pub pyth_spy: String,
    #[arg(long)]
    pub pyth_qqq: String,
    #[arg(long)]
    pub pyth_iwm: String,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let holder = ctx.signer()?;
    let policy = CliContext::parse_pubkey("policy", &args.policy)?;
    let usdc_mint = ctx.resolve_usdc_mint(args.usdc_mint.as_deref())?;
    let pyth_spy = CliContext::parse_pubkey("pyth_spy", &args.pyth_spy)?;
    let pyth_qqq = CliContext::parse_pubkey("pyth_qqq", &args.pyth_qqq)?;
    let pyth_iwm = CliContext::parse_pubkey("pyth_iwm", &args.pyth_iwm)?;
    let header =
        fetch_anchor_account::<halcyon_kernel::state::PolicyHeader>(ctx.rpc.as_ref(), &policy)
            .await?;

    let ixs = flagship_autocall::liquidate_wrapped_flagship_ixs(
        &holder.pubkey(),
        &usdc_mint,
        pyth_spy,
        pyth_qqq,
        pyth_iwm,
        &header,
        policy,
    );
    let signature = tx::send_instructions(ctx.rpc.as_ref(), holder, ixs).await?;
    println!("liquidate-wrapped-flagship: signature={signature}");
    println!("  policy={policy}");
    println!("  holder={}", holder.pubkey());
    Ok(())
}
