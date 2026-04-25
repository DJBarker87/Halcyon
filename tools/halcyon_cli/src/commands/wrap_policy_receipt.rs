use anyhow::{bail, Result};
use clap::Args as ClapArgs;
use solana_sdk::signer::Signer;

use halcyon_client_sdk::{decode::fetch_anchor_account, kernel, pda, tx};

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    pub policy: String,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let holder = ctx.signer()?;
    let policy = CliContext::parse_pubkey("policy", &args.policy)?;
    let header =
        fetch_anchor_account::<halcyon_kernel::state::PolicyHeader>(ctx.rpc.as_ref(), &policy)
            .await?;

    if header.owner != holder.pubkey() {
        bail!(
            "keypair {} is not the current policy owner {}",
            holder.pubkey(),
            header.owner
        );
    }

    let ix = kernel::wrap_policy_receipt_ix(&holder.pubkey(), &policy);
    let signature = tx::send_instructions(ctx.rpc.as_ref(), holder, vec![ix]).await?;
    let (receipt_mint, _) = pda::policy_receipt_mint(&policy);
    println!("wrap-policy-receipt: signature={signature}");
    println!("  policy={policy}");
    println!("  holder={}", holder.pubkey());
    println!("  receipt_mint={receipt_mint}");
    Ok(())
}
