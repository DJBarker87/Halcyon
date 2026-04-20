use anyhow::Result;

use halcyon_client_sdk::{decode::fetch_anchor_account_opt, pda};

use crate::client::CliContext;

pub async fn run(ctx: &CliContext) -> Result<()> {
    let slot = ctx.rpc.get_slot().await?;
    let (regime_signal_addr, _) = pda::regime_signal(&halcyon_il_protection::ID);
    match fetch_anchor_account_opt::<halcyon_kernel::state::RegimeSignal>(
        ctx.rpc.as_ref(),
        &regime_signal_addr,
    )
    .await?
    {
        Some(signal) => {
            println!("regime-status: slot={slot} account={regime_signal_addr}");
            println!("  product_program_id={}", signal.product_program_id);
            println!("  fvol_s6={}", signal.fvol_s6);
            println!("  regime={:?}", signal.regime);
            println!("  sigma_multiplier_s6={}", signal.sigma_multiplier_s6);
            println!(
                "  sigma_floor_annualised_s6={}",
                signal.sigma_floor_annualised_s6
            );
            println!("  last_update_ts={}", signal.last_update_ts);
            println!("  last_update_slot={}", signal.last_update_slot);
        }
        None => println!("regime-status: IL Protection RegimeSignal is not initialized"),
    }
    Ok(())
}
