use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use halcyon_client_sdk::{
    kernel::{self, SetProtocolConfigArgs},
    tx,
};
use halcyon_flagship_autocall::pricing::{
    DAILY_KI_CORRECTION_SHA256 as CURRENT_DAILY_KI_CORRECTION_SHA256,
    K12_CORRECTION_SHA256 as CURRENT_K12_CORRECTION_SHA256,
};
use halcyon_kernel::PremiumSplitsBps;
use halcyon_sol_autocall_quote::generated::pod_deim_table::POD_DEIM_TABLE_SHA256;
use solana_sdk::signer::Signer;

use crate::client::CliContext;

#[derive(Debug, Default, ClapArgs)]
pub struct Args {
    /// Set `ProtocolConfig.sigma_floor_annualised_s6`.
    #[arg(long)]
    pub sigma_floor_annualised_s6: Option<i64>,

    /// Set `ProtocolConfig.sigma_ceiling_annualised_s6`.
    #[arg(long)]
    pub sigma_ceiling_annualised_s6: Option<i64>,

    /// Set `ProtocolConfig.il_sigma_floor_annualised_s6`.
    #[arg(long)]
    pub il_sigma_floor_annualised_s6: Option<i64>,

    /// Set `ProtocolConfig.sol_autocall_sigma_floor_annualised_s6`.
    #[arg(long)]
    pub sol_autocall_sigma_floor_annualised_s6: Option<i64>,

    /// Set `ProtocolConfig.flagship_sigma_floor_annualised_s6`.
    #[arg(long)]
    pub flagship_sigma_floor_annualised_s6: Option<i64>,

    /// Set `ProtocolConfig.sigma_staleness_cap_secs`.
    #[arg(long)]
    pub sigma_staleness_cap_secs: Option<i64>,

    /// Set `ProtocolConfig.pyth_quote_staleness_cap_secs`.
    #[arg(long)]
    pub pyth_quote_staleness_cap_secs: Option<i64>,

    /// Set `ProtocolConfig.ewma_rate_limit_secs`.
    #[arg(long)]
    pub ewma_rate_limit_secs: Option<i64>,

    /// Set `ProtocolConfig.il_ewma_rate_limit_secs`.
    #[arg(long)]
    pub il_ewma_rate_limit_secs: Option<u64>,

    /// Set `ProtocolConfig.sol_autocall_ewma_rate_limit_secs`.
    #[arg(long)]
    pub sol_autocall_ewma_rate_limit_secs: Option<u64>,

    /// Set `ProtocolConfig.k12_correction_sha256` from a 64-char hex string.
    #[arg(long)]
    pub k12_correction_sha256: Option<String>,

    /// Set `ProtocolConfig.k12_correction_sha256` to the checked-in current table hash.
    #[arg(long, default_value_t = false)]
    pub k12_correction_sha256_current: bool,

    /// Set `ProtocolConfig.daily_ki_correction_sha256` from a 64-char hex string.
    #[arg(long)]
    pub daily_ki_correction_sha256: Option<String>,

    /// Set `ProtocolConfig.daily_ki_correction_sha256` to the checked-in current table hash.
    #[arg(long, default_value_t = false)]
    pub daily_ki_correction_sha256_current: bool,

    /// Set `ProtocolConfig.pod_deim_table_sha256` from a 64-char hex string.
    #[arg(long)]
    pub pod_deim_table_sha256: Option<String>,

    /// Set `ProtocolConfig.pod_deim_table_sha256` to the checked-in current table hash.
    #[arg(long, default_value_t = false)]
    pub pod_deim_table_sha256_current: bool,

    /// Set `ProtocolConfig.senior_share_bps` as part of the premium split.
    #[arg(long)]
    pub premium_senior_share_bps: Option<u16>,

    /// Set `ProtocolConfig.junior_share_bps` as part of the premium split.
    #[arg(long)]
    pub premium_junior_share_bps: Option<u16>,

    /// Set `ProtocolConfig.treasury_share_bps` as part of the premium split.
    #[arg(long)]
    pub premium_treasury_share_bps: Option<u16>,

    /// Simulate the config update without submitting it.
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,
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
    let k12_hash = match (
        args.k12_correction_sha256_current,
        args.k12_correction_sha256,
    ) {
        (true, Some(_)) => bail!(
            "choose exactly one of --k12-correction-sha256 or --k12-correction-sha256-current"
        ),
        (true, None) => Some(CURRENT_K12_CORRECTION_SHA256),
        (false, Some(value)) => Some(parse_sha256_hex(&value)?),
        (false, None) => None,
    };
    let daily_ki_hash = match (
        args.daily_ki_correction_sha256_current,
        args.daily_ki_correction_sha256,
    ) {
        (true, Some(_)) => bail!(
            "choose exactly one of --daily-ki-correction-sha256 or --daily-ki-correction-sha256-current"
        ),
        (true, None) => Some(CURRENT_DAILY_KI_CORRECTION_SHA256),
        (false, Some(value)) => Some(parse_sha256_hex(&value)?),
        (false, None) => None,
    };
    let pod_hash = match (
        args.pod_deim_table_sha256_current,
        args.pod_deim_table_sha256,
    ) {
        (true, Some(_)) => bail!(
            "choose exactly one of --pod-deim-table-sha256 or --pod-deim-table-sha256-current"
        ),
        (true, None) => Some(POD_DEIM_TABLE_SHA256),
        (false, Some(value)) => Some(parse_sha256_hex(&value)?),
        (false, None) => None,
    };
    let premium_splits_bps = match (
        args.premium_senior_share_bps,
        args.premium_junior_share_bps,
        args.premium_treasury_share_bps,
    ) {
        (None, None, None) => None,
        (Some(senior_bps), Some(junior_bps), Some(treasury_bps)) => Some(PremiumSplitsBps {
            senior_bps,
            junior_bps,
            treasury_bps,
        }),
        _ => bail!(
            "set all three premium split fields: --premium-senior-share-bps, --premium-junior-share-bps, --premium-treasury-share-bps"
        ),
    };
    if pod_hash.is_none()
        && k12_hash.is_none()
        && daily_ki_hash.is_none()
        && premium_splits_bps.is_none()
        && args.sigma_floor_annualised_s6.is_none()
        && args.sigma_ceiling_annualised_s6.is_none()
        && args.il_sigma_floor_annualised_s6.is_none()
        && args.sol_autocall_sigma_floor_annualised_s6.is_none()
        && args.flagship_sigma_floor_annualised_s6.is_none()
        && args.sigma_staleness_cap_secs.is_none()
        && args.pyth_quote_staleness_cap_secs.is_none()
        && args.ewma_rate_limit_secs.is_none()
        && args.il_ewma_rate_limit_secs.is_none()
        && args.sol_autocall_ewma_rate_limit_secs.is_none()
    {
        bail!("no protocol-config fields specified");
    }

    let ix = kernel::set_protocol_config_ix(
        &admin.pubkey(),
        SetProtocolConfigArgs {
            utilization_cap_bps: None,
            sigma_staleness_cap_secs: args.sigma_staleness_cap_secs,
            regime_staleness_cap_secs: None,
            regression_staleness_cap_secs: None,
            pyth_quote_staleness_cap_secs: args.pyth_quote_staleness_cap_secs,
            pyth_settle_staleness_cap_secs: None,
            quote_ttl_secs: None,
            ewma_rate_limit_secs: args.ewma_rate_limit_secs,
            il_ewma_rate_limit_secs: args.il_ewma_rate_limit_secs,
            sol_autocall_ewma_rate_limit_secs: args.sol_autocall_ewma_rate_limit_secs,
            senior_cooldown_secs: None,
            sigma_floor_annualised_s6: args.sigma_floor_annualised_s6,
            il_sigma_floor_annualised_s6: args.il_sigma_floor_annualised_s6,
            sol_autocall_sigma_floor_annualised_s6: args.sol_autocall_sigma_floor_annualised_s6,
            flagship_sigma_floor_annualised_s6: args.flagship_sigma_floor_annualised_s6,
            sigma_ceiling_annualised_s6: args.sigma_ceiling_annualised_s6,
            k12_correction_sha256: k12_hash,
            daily_ki_correction_sha256: daily_ki_hash,
            pod_deim_table_sha256: pod_hash,
            premium_splits_bps,
            sol_autocall_quote_config_bps: None,
            treasury_destination: None,
            hedge_max_slippage_bps_cap: None,
            hedge_defund_destination: None,
        },
    );
    if args.dry_run {
        let result = tx::simulate_instructions(ctx.rpc.as_ref(), admin, vec![ix], &[]).await?;
        println!(
            "set-protocol-config dry-run: units_consumed={}",
            result
                .units_consumed
                .map(|units| units.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        );
        return Ok(());
    }
    let sig = tx::send_instructions(ctx.rpc.as_ref(), admin, vec![ix]).await?;
    let mut fields = Vec::new();
    if let Some(value) = args.sigma_floor_annualised_s6 {
        fields.push(format!("sigma_floor_annualised_s6={value}"));
    }
    if let Some(value) = args.sigma_ceiling_annualised_s6 {
        fields.push(format!("sigma_ceiling_annualised_s6={value}"));
    }
    if let Some(value) = args.il_sigma_floor_annualised_s6 {
        fields.push(format!("il_sigma_floor_annualised_s6={value}"));
    }
    if let Some(value) = args.sol_autocall_sigma_floor_annualised_s6 {
        fields.push(format!("sol_autocall_sigma_floor_annualised_s6={value}"));
    }
    if let Some(value) = args.flagship_sigma_floor_annualised_s6 {
        fields.push(format!("flagship_sigma_floor_annualised_s6={value}"));
    }
    if let Some(value) = args.sigma_staleness_cap_secs {
        fields.push(format!("sigma_staleness_cap_secs={value}"));
    }
    if let Some(value) = args.pyth_quote_staleness_cap_secs {
        fields.push(format!("pyth_quote_staleness_cap_secs={value}"));
    }
    if let Some(value) = args.ewma_rate_limit_secs {
        fields.push(format!("ewma_rate_limit_secs={value}"));
    }
    if let Some(value) = args.il_ewma_rate_limit_secs {
        fields.push(format!("il_ewma_rate_limit_secs={value}"));
    }
    if let Some(value) = args.sol_autocall_ewma_rate_limit_secs {
        fields.push(format!("sol_autocall_ewma_rate_limit_secs={value}"));
    }
    if let (Some(senior), Some(junior), Some(treasury)) = (
        args.premium_senior_share_bps,
        args.premium_junior_share_bps,
        args.premium_treasury_share_bps,
    ) {
        fields.push(format!("premium_splits_bps={senior}/{junior}/{treasury}"));
    }
    if let Some(hash) = k12_hash {
        fields.push(format!(
            "k12_correction_sha256={}",
            format_sha256_hex(&hash)
        ));
    }
    if let Some(hash) = daily_ki_hash {
        fields.push(format!(
            "daily_ki_correction_sha256={}",
            format_sha256_hex(&hash)
        ));
    }
    if let Some(hash) = pod_hash {
        fields.push(format!(
            "pod_deim_table_sha256={}",
            format_sha256_hex(&hash)
        ));
    }
    println!("set-protocol-config: sig={sig} {}", fields.join(" "));
    Ok(())
}
