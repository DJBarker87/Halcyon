use anyhow::Result;
use clap::Args as ClapArgs;
use solana_sdk::signer::Signer;

use halcyon_client_sdk::{kernel, tx};

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    pub policy: String,
    #[arg(long)]
    pub new_owner: String,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let current_owner = ctx.signer()?;
    let policy = CliContext::parse_pubkey("policy", &args.policy)?;
    let new_owner = CliContext::parse_pubkey("new_owner", &args.new_owner)?;
    let ix = kernel::transfer_policy_owner_ix(&current_owner.pubkey(), &policy, new_owner);
    let signature = tx::send_instructions(ctx.rpc.as_ref(), current_owner, vec![ix]).await?;
    println!("transfer-policy-owner: signature={signature}");
    println!("  policy={policy}");
    println!("  old_owner={}", current_owner.pubkey());
    println!("  new_owner={new_owner}");
    Ok(())
}
