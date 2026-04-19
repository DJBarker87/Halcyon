use anyhow::Result;
use clap::Args as ClapArgs;
use halcyon_client_sdk::{kernel::rotate_keeper_ix, tx::send_instructions};
use solana_sdk::signer::Signer;

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    #[arg(long)]
    pub role: u8,
    #[arg(long)]
    pub new_authority: String,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let admin = ctx.signer()?;
    let new_authority = CliContext::parse_pubkey("new_authority", &args.new_authority)?;
    let ix = rotate_keeper_ix(&admin.pubkey(), args.role, new_authority);
    let sig = send_instructions(&ctx.rpc, admin, vec![ix]).await?;
    println!(
        "rotated keeper role {} to {}: sig={sig}",
        args.role, new_authority
    );
    Ok(())
}
