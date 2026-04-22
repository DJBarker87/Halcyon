use anyhow::Result;
use solana_sdk::pubkey::Pubkey;

use halcyon_client_sdk::{
    decode::{
        fetch_anchor_account, fetch_anchor_account_opt, fetch_multiple_accounts,
        list_policy_headers_for_product,
    },
    pda,
};

use crate::client::CliContext;

fn format_sha256_hex(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub async fn run(ctx: &CliContext) -> Result<()> {
    let slot = ctx.rpc.get_slot().await?;
    let (protocol_config_addr, _) = pda::protocol_config();
    let (vault_state_addr, _) = pda::vault_state();
    let (fee_ledger_addr, _) = pda::fee_ledger();
    let (keeper_registry_addr, _) = pda::keeper_registry();
    let (product_registry_addr, _) = pda::product_registry_entry(&halcyon_sol_autocall::ID);
    let (coupon_vault_addr, _) = pda::coupon_vault(&halcyon_sol_autocall::ID);
    let (hedge_sleeve_addr, _) = pda::hedge_sleeve(&halcyon_sol_autocall::ID);
    let (hedge_book_addr, _) = pda::hedge_book(&halcyon_sol_autocall::ID);

    let protocol_config = fetch_anchor_account::<halcyon_kernel::state::ProtocolConfig>(
        ctx.rpc.as_ref(),
        &protocol_config_addr,
    )
    .await?;
    let treasury_destination = ctx
        .rpc
        .get_account(&protocol_config.treasury_destination)
        .await?;
    let mint_bytes: [u8; 32] = treasury_destination
        .data
        .get(..32)
        .ok_or_else(|| anyhow::anyhow!("treasury_destination is not a token account"))?
        .try_into()
        .map_err(|_| anyhow::anyhow!("treasury_destination is not a token account"))?;
    let usdc_mint = Pubkey::new_from_array(mint_bytes);
    let hedge_sleeve_usdc_addr = pda::hedge_sleeve_usdc(&halcyon_sol_autocall::ID, &usdc_mint);
    let vault_state = fetch_anchor_account::<halcyon_kernel::state::VaultState>(
        ctx.rpc.as_ref(),
        &vault_state_addr,
    )
    .await?;
    let fee_ledger = fetch_anchor_account::<halcyon_kernel::state::FeeLedger>(
        ctx.rpc.as_ref(),
        &fee_ledger_addr,
    )
    .await?;
    let keeper_registry = fetch_anchor_account::<halcyon_kernel::state::KeeperRegistry>(
        ctx.rpc.as_ref(),
        &keeper_registry_addr,
    )
    .await?;
    let product_registry = fetch_anchor_account::<halcyon_kernel::state::ProductRegistryEntry>(
        ctx.rpc.as_ref(),
        &product_registry_addr,
    )
    .await?;
    let coupon_vault = fetch_anchor_account_opt::<halcyon_kernel::state::CouponVault>(
        ctx.rpc.as_ref(),
        &coupon_vault_addr,
    )
    .await?;
    let hedge_sleeve = fetch_anchor_account_opt::<halcyon_kernel::state::HedgeSleeve>(
        ctx.rpc.as_ref(),
        &hedge_sleeve_addr,
    )
    .await?;
    let hedge_book = fetch_anchor_account_opt::<halcyon_kernel::state::HedgeBookState>(
        ctx.rpc.as_ref(),
        &hedge_book_addr,
    )
    .await?;
    let hedge_sleeve_usdc_balance = ctx
        .rpc
        .get_token_account_balance(&hedge_sleeve_usdc_addr)
        .await
        .ok()
        .map(|balance| balance.amount);

    let mut policies =
        list_policy_headers_for_product(ctx.rpc.as_ref(), &halcyon_sol_autocall::ID).await?;
    policies.sort_by_key(|(_, header)| header.issued_at);
    let active: Vec<_> = policies
        .into_iter()
        .filter(|(_, header)| header.status == halcyon_kernel::state::PolicyStatus::Active)
        .collect();
    let terms_addresses: Vec<_> = active
        .iter()
        .map(|(_, header)| header.product_terms)
        .collect();
    let term_accounts = fetch_multiple_accounts(ctx.rpc.as_ref(), &terms_addresses).await?;

    println!("status: slot={slot}");
    println!(
        "protocol: admin={} issuance_paused={} settlement_paused={} utilization_cap_bps={} sigma_floor_annualised_s6={} sigma_ceiling_annualised_s6={}",
        protocol_config.admin,
        protocol_config.issuance_paused_global,
        protocol_config.settlement_paused_global,
        protocol_config.utilization_cap_bps,
        protocol_config.sigma_floor_annualised_s6,
        protocol_config.sigma_ceiling_annualised_s6
    );
    println!(
        "sigma_floors_annualised_s6: fallback={} il={} sol_autocall={} flagship={} ceiling={}",
        protocol_config.sigma_floor_annualised_s6,
        protocol_config.il_sigma_floor_annualised_s6,
        protocol_config.sol_autocall_sigma_floor_annualised_s6,
        protocol_config.flagship_sigma_floor_annualised_s6,
        protocol_config.sigma_ceiling_annualised_s6
    );
    println!(
        "ewma_rate_limits_secs: fallback={} il={} sol_autocall={}",
        protocol_config.ewma_rate_limit_secs,
        protocol_config.il_ewma_rate_limit_secs,
        protocol_config.sol_autocall_ewma_rate_limit_secs
    );
    println!(
        "staleness_caps_secs: sigma={} regression={} pyth_quote={} pyth_settle={} quote_ttl={}",
        protocol_config.sigma_staleness_cap_secs,
        protocol_config.regression_staleness_cap_secs,
        protocol_config.pyth_quote_staleness_cap_secs,
        protocol_config.pyth_settle_staleness_cap_secs,
        protocol_config.quote_ttl_secs
    );
    println!(
        "correction_hashes: k12={} daily_ki={} pod_deim={}",
        format_sha256_hex(&protocol_config.k12_correction_sha256),
        format_sha256_hex(&protocol_config.daily_ki_correction_sha256),
        format_sha256_hex(&protocol_config.pod_deim_table_sha256)
    );
    println!(
        "vault: total_senior={} total_junior={} total_reserved_liability={} lifetime_premium_received={}",
        vault_state.total_senior,
        vault_state.total_junior,
        vault_state.total_reserved_liability,
        vault_state.lifetime_premium_received
    );
    println!(
        "fees: treasury_balance={} bucket_count={}",
        fee_ledger.treasury_balance, fee_ledger.bucket_count
    );
    println!(
        "keepers: observation={} hedge={} regime={} regression={}",
        keeper_registry.observation,
        keeper_registry.hedge,
        keeper_registry.regime,
        keeper_registry.regression
    );
    println!(
        "product: active={} paused={} total_reserved={} per_policy_risk_cap={} global_risk_cap={}",
        product_registry.active,
        product_registry.paused,
        product_registry.total_reserved,
        product_registry.per_policy_risk_cap,
        product_registry.global_risk_cap
    );
    match coupon_vault {
        Some(vault) => {
            println!(
                "coupon_vault: usdc_balance={} lifetime_coupons_paid={} last_update_ts={}",
                vault.usdc_balance, vault.lifetime_coupons_paid, vault.last_update_ts
            );
        }
        None => println!("coupon_vault: not initialized"),
    }
    match hedge_sleeve {
        Some(sleeve) => {
            println!(
                "hedge_sleeve: usdc_reserve={} actual_usdc_balance={} cumulative_funded_usdc={} cumulative_defunded_usdc={} lifetime_execution_cost={} last_funded_ts={} last_defunded_ts={} last_update_ts={}",
                sleeve.usdc_reserve,
                hedge_sleeve_usdc_balance
                    .as_deref()
                    .unwrap_or("<missing>"),
                sleeve.cumulative_funded_usdc,
                sleeve.cumulative_defunded_usdc,
                sleeve.lifetime_execution_cost,
                sleeve.last_funded_ts,
                sleeve.last_defunded_ts,
                sleeve.last_update_ts
            );
        }
        None => println!("hedge_sleeve: not initialized"),
    }
    match hedge_book {
        Some(book) => {
            let leg = book.legs[0];
            println!(
                "hedge_book: sequence={} leg0_position_raw={} leg0_target_raw={} cumulative_execution_cost={}",
                book.sequence,
                leg.current_position_raw,
                leg.target_position_raw,
                book.cumulative_execution_cost
            );
        }
        None => println!("hedge_book: not initialized"),
    }
    println!("active_policies={}", active.len());
    for ((policy_addr, header), term_account) in active.iter().zip(term_accounts.into_iter()) {
        let line = match term_account {
            Some(account) => {
                let terms = halcyon_client_sdk::decode::decode_anchor_account::<
                    halcyon_sol_autocall::state::SolAutocallTerms,
                >(&account.data)?;
                let next_obs = terms
                    .observation_schedule
                    .get(terms.current_observation_index as usize)
                    .copied()
                    .unwrap_or(header.expiry_ts);
                format!(
                    "  policy={} owner={} notional={} premium={} max_liability={} obs_index={} next_obs_ts={} coupon_bps_s6={} ki_triggered={}",
                    policy_addr,
                    header.owner,
                    header.notional,
                    header.premium_paid,
                    header.max_liability,
                    terms.current_observation_index,
                    next_obs,
                    terms.offered_coupon_bps_s6,
                    terms.ki_triggered
                )
            }
            None => format!(
                "  policy={} owner={} notional={} premium={} max_liability={} product_terms=<missing>",
                policy_addr, header.owner, header.notional, header.premium_paid, header.max_liability
            ),
        };
        println!("{line}");
    }
    Ok(())
}
