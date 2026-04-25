use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use solana_sdk::signer::Signer;

use halcyon_client_sdk::{decode::fetch_anchor_account, kernel, pda, tx};

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

    // M-2 — destination is pinned on-chain to
    // `protocol_config.hedge_defund_destination`. Read it so the CLI
    // constructs the correct ix without the operator having to pass it.
    let (protocol_config_pda, _) = pda::protocol_config();
    let protocol_config: halcyon_kernel::state::ProtocolConfig =
        fetch_anchor_account(ctx.rpc.as_ref(), &protocol_config_pda)
            .await
            .context("fetching protocol_config for hedge_defund_destination")?;
    let destination_usdc = protocol_config.hedge_defund_destination;

    let ix = kernel::defund_hedge_sleeve_ix(
        &admin.pubkey(),
        &usdc_mint,
        &halcyon_sol_autocall::ID,
        &destination_usdc,
        args.amount,
    );
    let signature = tx::send_instructions(ctx.rpc.as_ref(), admin, vec![ix]).await?;
    println!(
        "defund-sleeve: signature={signature} amount={} destination={} product_program_id={} hedge_sleeve={} hedge_sleeve_usdc={}",
        args.amount,
        destination_usdc,
        halcyon_sol_autocall::ID,
        pda::hedge_sleeve(&halcyon_sol_autocall::ID).0,
        pda::hedge_sleeve_usdc(&halcyon_sol_autocall::ID, &usdc_mint)
    );
    Ok(())
}
