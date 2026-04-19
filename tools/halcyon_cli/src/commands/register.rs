use anyhow::Result;
use clap::Args as ClapArgs;
use halcyon_client_sdk::{
    decode::fetch_anchor_account_opt, kernel::register_sol_autocall_ix, pda, tx::send_instructions,
};
use solana_sdk::signer::Signer;

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    #[arg(long, default_value_t = 500_000_000)]
    pub per_policy_risk_cap: u64,
    #[arg(long, default_value_t = 5_000_000_000)]
    pub global_risk_cap: u64,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let admin = ctx.signer()?;
    let (product_registry_entry, _) = pda::product_registry_entry(&halcyon_sol_autocall::ID);
    if fetch_anchor_account_opt::<halcyon_kernel::state::ProductRegistryEntry>(
        &ctx.rpc,
        &product_registry_entry,
    )
    .await?
    .is_some()
    {
        println!("SOL Autocall already registered at {product_registry_entry}");
        return Ok(());
    }

    let ix = register_sol_autocall_ix(
        &admin.pubkey(),
        args.per_policy_risk_cap,
        args.global_risk_cap,
    );
    let sig = send_instructions(&ctx.rpc, admin, vec![ix]).await?;
    println!(
        "registered SOL Autocall: sig={sig} registry={} sigma={}",
        product_registry_entry,
        pda::vault_sigma(&halcyon_sol_autocall::ID).0,
    );
    Ok(())
}
