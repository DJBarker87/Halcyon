use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use halcyon_client_sdk::{
    kernel::{self, SetProtocolConfigArgs},
    tx,
};
use halcyon_sol_autocall_quote::generated::pod_deim_table::POD_DEIM_TABLE_SHA256;
use solana_sdk::signer::Signer;

use crate::client::CliContext;

#[derive(Debug, Default, ClapArgs)]
pub struct Args {
    /// Set `ProtocolConfig.pod_deim_table_sha256` from a 64-char hex string.
    #[arg(long)]
    pub pod_deim_table_sha256: Option<String>,

    /// Set `ProtocolConfig.pod_deim_table_sha256` to the checked-in current table hash.
    #[arg(long, default_value_t = false)]
    pub pod_deim_table_sha256_current: bool,
}

fn parse_sha256_hex(input: &str) -> Result<[u8; 32]> {
    let trimmed = input.trim();
    let hex = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    if hex.len() != 64 {
        bail!("expected 64 hex chars for sha256, got {}", hex.len());
    }
    let mut out = [0u8; 32];
    for (idx, chunk) in hex.as_bytes().chunks_exact(2).enumerate() {
        let hi = (chunk[0] as char)
            .to_digit(16)
            .with_context(|| format!("invalid hex at byte {}", idx * 2))?;
        let lo = (chunk[1] as char)
            .to_digit(16)
            .with_context(|| format!("invalid hex at byte {}", idx * 2 + 1))?;
        out[idx] = ((hi << 4) | lo) as u8;
    }
    Ok(out)
}

fn format_sha256_hex(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let admin = ctx.signer()?;
    let pod_hash = match (args.pod_deim_table_sha256_current, args.pod_deim_table_sha256) {
        (true, Some(_)) => bail!(
            "choose exactly one of --pod-deim-table-sha256 or --pod-deim-table-sha256-current"
        ),
        (true, None) => Some(POD_DEIM_TABLE_SHA256),
        (false, Some(value)) => Some(parse_sha256_hex(&value)?),
        (false, None) => None,
    };
    if pod_hash.is_none() {
        bail!("no protocol-config fields specified");
    }

    let ix = kernel::set_protocol_config_ix(
        &admin.pubkey(),
        SetProtocolConfigArgs {
            utilization_cap_bps: None,
            sigma_staleness_cap_secs: None,
            regime_staleness_cap_secs: None,
            regression_staleness_cap_secs: None,
            pyth_quote_staleness_cap_secs: None,
            pyth_settle_staleness_cap_secs: None,
            quote_ttl_secs: None,
            ewma_rate_limit_secs: None,
            senior_cooldown_secs: None,
            sigma_floor_annualised_s6: None,
            k12_correction_sha256: None,
            daily_ki_correction_sha256: None,
            pod_deim_table_sha256: pod_hash,
            premium_splits_bps: None,
            sol_autocall_quote_config_bps: None,
            treasury_destination: None,
            hedge_max_slippage_bps_cap: None,
            hedge_defund_destination: None,
        },
    );
    let sig = tx::send_instructions(ctx.rpc.as_ref(), admin, vec![ix]).await?;
    let pod_hash = pod_hash.expect("checked above");
    println!(
        "set-protocol-config: sig={sig} pod_deim_table_sha256={}",
        format_sha256_hex(&pod_hash)
    );
    Ok(())
}
