use anyhow::Result;
use clap::Args as ClapArgs;
use solana_sdk::signer::Signer;

use halcyon_client_sdk::{kernel, tx};

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    pub policy: String,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let holder = ctx.signer()?;
    let policy = CliContext::parse_pubkey("policy", &args.policy)?;
    let ix = kernel::unwrap_policy_receipt_ix(&holder.pubkey(), &policy);
    let signature = tx::send_instructions(ctx.rpc.as_ref(), holder, vec![ix]).await?;
    println!("unwrap-policy-receipt: signature={signature}");
    println!("  policy={policy}");
    println!("  holder={}", holder.pubkey());
    Ok(())
}
