use anyhow::{anyhow, bail, Result};
use clap::Args as ClapArgs;
use solana_sdk::{instruction::Instruction, signature::Keypair, signer::Signer};

use halcyon_client_sdk::{decode::fetch_anchor_account, flagship_autocall, tx};

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    pub policy: String,
    #[arg(long)]
    pub pyth_spy: String,
    #[arg(long)]
    pub pyth_qqq: String,
    #[arg(long)]
    pub pyth_iwm: String,
    /// Use the legacy one-shot simulation instead of the checkpointed production path.
    #[arg(long)]
    pub direct: bool,
    /// Largest monthly coupon chunk to try while planning checkpoint txs.
    #[arg(long, default_value_t = flagship_autocall::MIDLIFE_CHECKPOINT_MAX_CHUNK_SIZE)]
    pub checkpoint_chunk_size: u8,
    /// Soft CU target used by the preflight chunk planner.
    #[arg(long, default_value_t = flagship_autocall::MIDLIFE_CHECKPOINT_TARGET_UNITS)]
    pub checkpoint_target_units: u64,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let policy = CliContext::parse_pubkey("policy", &args.policy)?;
    let pyth_spy = CliContext::parse_pubkey("pyth_spy", &args.pyth_spy)?;
    let pyth_qqq = CliContext::parse_pubkey("pyth_qqq", &args.pyth_qqq)?;
    let pyth_iwm = CliContext::parse_pubkey("pyth_iwm", &args.pyth_iwm)?;
    let header =
        fetch_anchor_account::<halcyon_kernel::state::PolicyHeader>(ctx.rpc.as_ref(), &policy)
            .await?;
    if !args.direct {
        if let Some(signer) = ctx.signer.as_ref() {
            let terms = fetch_anchor_account::<
                halcyon_flagship_autocall::state::FlagshipAutocallTerms,
            >(ctx.rpc.as_ref(), &header.product_terms)
            .await?;
            let result = checkpointed_preview_lending_value(
                ctx,
                signer,
                &header,
                &terms,
                policy,
                pyth_spy,
                pyth_qqq,
                pyth_iwm,
                args.checkpoint_chunk_size,
                args.checkpoint_target_units,
            )
            .await?;
            print_preview(
                "checkpointed",
                policy,
                header.owner,
                header.notional,
                &result.preview,
            );
            println!("  checkpoint={}", result.checkpoint);
            println!("  signatures={}", result.signatures.join(","));
            println!("  prepare_chunk_size={}", result.prepare_chunk_size);
            println!(
                "  advance_chunk_sizes={}",
                result
                    .advance_chunk_sizes
                    .iter()
                    .map(u8::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            );
            println!(
                "  transaction_units_consumed={}",
                result
                    .transaction_units_consumed
                    .iter()
                    .map(u64::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            );
            println!("  max_units_consumed={}", result.max_units_consumed);
            return Ok(());
        }
    }

    let ix = flagship_autocall::preview_lending_value_ix(
        halcyon_client_sdk::pda::protocol_config().0,
        halcyon_client_sdk::pda::vault_sigma(&halcyon_flagship_autocall::ID).0,
        halcyon_client_sdk::pda::regression().0,
        policy,
        header.product_terms,
        pyth_spy,
        pyth_qqq,
        pyth_iwm,
    );
    let ephemeral = Keypair::new();
    let payer = ctx.signer.as_ref().unwrap_or(&ephemeral);
    let result = tx::simulate_instruction(ctx.rpc.as_ref(), payer, ix).await?;
    let preview: flagship_autocall::LendingValuePreview =
        tx::decode_return_data(result, &halcyon_flagship_autocall::ID)?;

    print_preview(
        if args.direct {
            "direct"
        } else {
            "direct-no-keypair"
        },
        policy,
        header.owner,
        header.notional,
        &preview,
    );
    Ok(())
}

struct CheckpointedPreviewResult {
    preview: flagship_autocall::LendingValuePreview,
    checkpoint: solana_sdk::pubkey::Pubkey,
    signatures: Vec<String>,
    prepare_chunk_size: u8,
    advance_chunk_sizes: Vec<u8>,
    transaction_units_consumed: Vec<u64>,
    max_units_consumed: u64,
}

async fn checkpointed_preview_lending_value(
    ctx: &CliContext,
    signer: &Keypair,
    header: &halcyon_kernel::state::PolicyHeader,
    terms: &halcyon_flagship_autocall::state::FlagshipAutocallTerms,
    policy: solana_sdk::pubkey::Pubkey,
    pyth_spy: solana_sdk::pubkey::Pubkey,
    pyth_qqq: solana_sdk::pubkey::Pubkey,
    pyth_iwm: solana_sdk::pubkey::Pubkey,
    max_chunk_size: u8,
    target_units: u64,
) -> Result<CheckpointedPreviewResult> {
    if max_chunk_size == 0 || max_chunk_size > flagship_autocall::MIDLIFE_FINAL_COUPON_INDEX {
        bail!(
            "checkpoint_chunk_size must be in [1, {}]",
            flagship_autocall::MIDLIFE_FINAL_COUPON_INDEX
        );
    }
    if target_units == 0 || target_units > 1_400_000 {
        bail!("checkpoint_target_units must be in [1, 1400000]");
    }

    let checkpoint = Keypair::new();
    let rent = ctx
        .rpc
        .get_minimum_balance_for_rent_exemption(
            flagship_autocall::MIDLIFE_NAV_CHECKPOINT_ACCOUNT_SPACE,
        )
        .await?;
    let candidates = flagship_autocall::checkpoint_chunk_candidates(max_chunk_size);
    let mut current_coupon_index = terms.next_coupon_index;
    let mut signatures = Vec::new();
    let mut transaction_units_consumed = Vec::new();
    let mut advance_chunk_sizes = Vec::new();

    let mut selected_prepare: Option<(
        Vec<Instruction>,
        flagship_autocall::MidlifeNavCheckpointPreview,
        u64,
        u8,
    )> = None;
    let mut last_prepare_error: Option<anyhow::Error> = None;
    for chunk_size in &candidates {
        let stop_coupon_index =
            flagship_autocall::next_midlife_checkpoint_stop(current_coupon_index, *chunk_size);
        let instructions = vec![
            flagship_autocall::create_midlife_checkpoint_account_ix(
                &signer.pubkey(),
                &checkpoint.pubkey(),
                rent,
            ),
            flagship_autocall::prepare_midlife_nav_ix(
                &signer.pubkey(),
                checkpoint.pubkey(),
                pyth_spy,
                pyth_qqq,
                pyth_iwm,
                header,
                policy,
                stop_coupon_index,
            ),
        ];
        match tx::simulate_instructions(
            ctx.rpc.as_ref(),
            signer,
            instructions.clone(),
            &[&checkpoint],
        )
        .await
        .and_then(|result| {
            let units = result.units_consumed.unwrap_or(1_400_000);
            let preview: flagship_autocall::MidlifeNavCheckpointPreview =
                tx::decode_return_data(result, &halcyon_flagship_autocall::ID)?;
            Ok((preview, units))
        }) {
            Ok((preview, units)) if units <= target_units => {
                selected_prepare = Some((instructions, preview, units, *chunk_size));
                break;
            }
            Ok((_preview, units)) => {
                last_prepare_error = Some(anyhow!(
                    "prepare chunk {chunk_size} used {units} CU, above target {target_units}"
                ));
            }
            Err(error) => {
                last_prepare_error = Some(error);
            }
        }
    }

    let Some((prepare_ixs, prepare_preview, prepare_units, prepare_chunk_size)) = selected_prepare
    else {
        return Err(
            last_prepare_error.unwrap_or_else(|| anyhow!("no checkpoint prepare candidate"))
        );
    };
    let signature = tx::send_compute_instructions_with_extra_signers(
        ctx.rpc.as_ref(),
        signer,
        prepare_ixs,
        &[&checkpoint],
    )
    .await?;
    signatures.push(signature.to_string());
    transaction_units_consumed.push(prepare_units);
    current_coupon_index = prepare_preview.next_coupon_index;

    let mut pending_advances: Vec<Instruction> = Vec::new();
    while current_coupon_index < flagship_autocall::MIDLIFE_FINAL_COUPON_INDEX {
        let mut selected_advance: Option<(
            Instruction,
            flagship_autocall::MidlifeNavCheckpointPreview,
            u8,
        )> = None;
        for chunk_size in &candidates {
            let stop_coupon_index =
                flagship_autocall::next_midlife_checkpoint_stop(current_coupon_index, *chunk_size);
            let ix = flagship_autocall::advance_midlife_nav_ix(
                &signer.pubkey(),
                checkpoint.pubkey(),
                header,
                policy,
                stop_coupon_index,
            );
            let mut trial = pending_advances.clone();
            trial.push(ix.clone());
            let Ok((preview, units)) =
                tx::simulate_instructions(ctx.rpc.as_ref(), signer, trial, &[])
                    .await
                    .and_then(|result| {
                        let units = result.units_consumed.unwrap_or(1_400_000);
                        let preview: flagship_autocall::MidlifeNavCheckpointPreview =
                            tx::decode_return_data(result, &halcyon_flagship_autocall::ID)?;
                        Ok((preview, units))
                    })
            else {
                continue;
            };
            if units <= target_units {
                selected_advance = Some((ix, preview, *chunk_size));
                break;
            }
        }

        if let Some((ix, preview, chunk_size)) = selected_advance {
            pending_advances.push(ix);
            advance_chunk_sizes.push(chunk_size);
            current_coupon_index = preview.next_coupon_index;
            continue;
        }

        if pending_advances.is_empty() {
            bail!("unable to advance checkpoint at coupon {current_coupon_index}");
        }
        send_pending_advances(
            ctx,
            signer,
            &mut pending_advances,
            target_units,
            &mut signatures,
            &mut transaction_units_consumed,
        )
        .await?;
    }

    let finish_ix = flagship_autocall::preview_lending_value_from_checkpoint_ix(
        &signer.pubkey(),
        checkpoint.pubkey(),
        header,
        policy,
    );
    let mut finish_ixs = pending_advances.clone();
    finish_ixs.push(finish_ix.clone());
    let mut finish = simulate_lending_preview(ctx, signer, finish_ixs.clone())
        .await
        .ok();
    if finish
        .as_ref()
        .map(|(_, units)| *units > target_units)
        .unwrap_or(true)
        && !pending_advances.is_empty()
    {
        send_pending_advances(
            ctx,
            signer,
            &mut pending_advances,
            target_units,
            &mut signatures,
            &mut transaction_units_consumed,
        )
        .await?;
        finish_ixs = vec![finish_ix];
        finish = Some(simulate_lending_preview(ctx, signer, finish_ixs.clone()).await?);
    }

    let Some((preview, finish_units)) = finish else {
        bail!("unable to finish checkpoint");
    };
    if finish_units > target_units {
        bail!("checkpoint finish used {finish_units} CU, above target {target_units}");
    }
    let signature =
        tx::send_compute_instructions_with_extra_signers(ctx.rpc.as_ref(), signer, finish_ixs, &[])
            .await?;
    signatures.push(signature.to_string());
    transaction_units_consumed.push(finish_units);
    let max_units_consumed = transaction_units_consumed
        .iter()
        .copied()
        .max()
        .unwrap_or(0);

    Ok(CheckpointedPreviewResult {
        preview,
        checkpoint: checkpoint.pubkey(),
        signatures,
        prepare_chunk_size,
        advance_chunk_sizes,
        transaction_units_consumed,
        max_units_consumed,
    })
}

async fn send_pending_advances(
    ctx: &CliContext,
    signer: &Keypair,
    pending_advances: &mut Vec<Instruction>,
    target_units: u64,
    signatures: &mut Vec<String>,
    transaction_units_consumed: &mut Vec<u64>,
) -> Result<()> {
    if pending_advances.is_empty() {
        return Ok(());
    }
    let result =
        tx::simulate_instructions(ctx.rpc.as_ref(), signer, pending_advances.clone(), &[]).await?;
    let units = result.units_consumed.unwrap_or(1_400_000);
    if units > target_units {
        bail!("checkpoint advance batch used {units} CU, above target {target_units}");
    }
    let signature = tx::send_compute_instructions_with_extra_signers(
        ctx.rpc.as_ref(),
        signer,
        std::mem::take(pending_advances),
        &[],
    )
    .await?;
    signatures.push(signature.to_string());
    transaction_units_consumed.push(units);
    Ok(())
}

async fn simulate_lending_preview(
    ctx: &CliContext,
    signer: &Keypair,
    instructions: Vec<Instruction>,
) -> Result<(flagship_autocall::LendingValuePreview, u64)> {
    let result = tx::simulate_instructions(ctx.rpc.as_ref(), signer, instructions, &[]).await?;
    let units = result.units_consumed.unwrap_or(1_400_000);
    let preview: flagship_autocall::LendingValuePreview =
        tx::decode_return_data(result, &halcyon_flagship_autocall::ID)?;
    Ok((preview, units))
}

fn print_preview(
    mode: &str,
    policy: solana_sdk::pubkey::Pubkey,
    owner: solana_sdk::pubkey::Pubkey,
    notional: u64,
    preview: &flagship_autocall::LendingValuePreview,
) {
    println!("preview-lending-value:");
    println!("  mode={mode}");
    println!("  policy={policy}");
    println!("  owner={owner}");
    println!("  notional_usdc={notional}");
    println!("  nav_s6={}", preview.nav_s6);
    println!("  ki_level_s6={}", preview.ki_level_usd_s6);
    println!("  lending_value_s6={}", preview.lending_value_s6);
    println!(
        "  lending_value_payout_usdc={}",
        preview.lending_value_payout_usdc
    );
    println!(
        "  remaining_coupon_pv_s6={}",
        preview.remaining_coupon_pv_s6
    );
    println!(
        "  par_recovery_probability_s6={}",
        preview.par_recovery_probability_s6
    );
    println!("  sigma_pricing_s6={}", preview.sigma_pricing_s6);
    println!("  now_trading_day={}", preview.now_trading_day);
}
