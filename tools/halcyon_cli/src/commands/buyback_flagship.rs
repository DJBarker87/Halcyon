use anyhow::{bail, Result};
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
    let owner = ctx.signer()?;
    let policy = CliContext::parse_pubkey("policy", &args.policy)?;
    let usdc_mint = ctx.resolve_usdc_mint(args.usdc_mint.as_deref())?;
    let pyth_spy = CliContext::parse_pubkey("pyth_spy", &args.pyth_spy)?;
    let pyth_qqq = CliContext::parse_pubkey("pyth_qqq", &args.pyth_qqq)?;
    let pyth_iwm = CliContext::parse_pubkey("pyth_iwm", &args.pyth_iwm)?;
    let header =
        fetch_anchor_account::<halcyon_kernel::state::PolicyHeader>(ctx.rpc.as_ref(), &policy)
            .await?;

    if header.owner != owner.pubkey() {
        bail!(
            "keypair {} is not the current policy owner {}",
            owner.pubkey(),
            header.owner
        );
    }

    let ix = flagship_autocall::buyback_ix(
        &owner.pubkey(),
        &usdc_mint,
        pyth_spy,
        pyth_qqq,
        pyth_iwm,
        &header,
        policy,
    );
    let signature = tx::send_instructions(ctx.rpc.as_ref(), owner, vec![ix]).await?;
    println!("buyback-flagship: signature={signature}");
    println!("  policy={policy}");
    println!("  owner={}", owner.pubkey());
    Ok(())
}
