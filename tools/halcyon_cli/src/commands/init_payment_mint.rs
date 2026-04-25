use anyhow::Result;
use clap::Args as ClapArgs;
use solana_sdk::signer::Signer;

use halcyon_client_sdk::{kernel, pda, tx};

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    #[arg(long, value_name = "PUBKEY")]
    pub usdc_mint: Option<String>,
    /// Leave the existing treasury/hedge-defund destinations unchanged.
    #[arg(long, default_value_t = false)]
    pub keep_destinations: bool,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let admin = ctx.signer()?;
    let usdc_mint = ctx.resolve_usdc_mint(args.usdc_mint.as_deref())?;
    let ix = kernel::initialize_payment_mint_ix(
        &admin.pubkey(),
        &usdc_mint,
        kernel::InitializePaymentMintArgs {
            set_admin_destinations: !args.keep_destinations,
        },
    );
    let signature = tx::send_instructions(ctx.rpc.as_ref(), admin, vec![ix]).await?;
    let (vault_usdc, _) = pda::vault_usdc(&usdc_mint);
    let (treasury_usdc, _) = pda::treasury_usdc(&usdc_mint);
    let admin_usdc = pda::associated_token_account(&admin.pubkey(), &usdc_mint);
    println!("init-payment-mint: signature={signature}");
    println!("  usdc_mint={usdc_mint}");
    println!("  vault_usdc={vault_usdc}");
    println!("  treasury_usdc={treasury_usdc}");
    println!("  admin_usdc={admin_usdc}");
    println!(
        "  destinations_rotated={}",
        if args.keep_destinations {
            "false"
        } else {
            "true"
        }
    );
    Ok(())
}
