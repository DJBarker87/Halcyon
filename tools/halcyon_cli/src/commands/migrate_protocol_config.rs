use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use halcyon_client_sdk::{kernel, pda, tx};
use solana_sdk::signer::Signer;

use crate::client::CliContext;

#[derive(Debug, Default, ClapArgs)]
pub struct Args {}

pub async fn run(ctx: &CliContext, _args: Args) -> Result<()> {
    let admin = ctx.signer()?;
    let (protocol_config, _) = pda::protocol_config();
    let before_len = ctx
        .rpc
        .get_account(&protocol_config)
        .await
        .context("fetching protocol_config before migration")?
        .data
        .len();
    let ix = kernel::migrate_protocol_config_ix(&admin.pubkey());
    let sig = tx::send_instructions(ctx.rpc.as_ref(), admin, vec![ix]).await?;
    let after_len = ctx
        .rpc
        .get_account(&protocol_config)
        .await
        .context("fetching protocol_config after migration")?
        .data
        .len();
    println!(
        "migrate-protocol-config: sig={sig} protocol_config={protocol_config} len={before_len}->{after_len}"
    );
    Ok(())
}
