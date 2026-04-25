use anyhow::{bail, Result};
use clap::Args as ClapArgs;
use solana_sdk::signer::Signer;

use halcyon_client_sdk::{decode::fetch_anchor_account, flagship_autocall, tx};

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    pub policy: String,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let owner = ctx.signer()?;
    let policy = CliContext::parse_pubkey("policy", &args.policy)?;
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

    let ix = flagship_autocall::cancel_retail_redemption_ix(&owner.pubkey(), &header, policy);
    let signature = tx::send_instructions(ctx.rpc.as_ref(), owner, vec![ix]).await?;
    println!("cancel-retail-redemption: signature={signature}");
    println!("  policy={policy}");
    println!("  owner={}", owner.pubkey());
    Ok(())
}
