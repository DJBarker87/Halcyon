use anyhow::{anyhow, Context, Result};
use base64::Engine;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client_api::config::RpcSimulateTransactionConfig;
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount,
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    message::{v0::Message as MessageV0, VersionedMessage},
    signature::{Keypair, Signature},
    signer::Signer,
    transaction::{Transaction, VersionedTransaction},
};

fn simulate_heap_frame_bytes() -> Result<Option<u32>> {
    let Some(raw) = std::env::var_os("HALCYON_SIM_HEAP_FRAME_BYTES") else {
        return Ok(None);
    };
    let parsed: u32 = raw
        .to_string_lossy()
        .parse()
        .context("parsing HALCYON_SIM_HEAP_FRAME_BYTES as u32")?;
    Ok(Some(parsed))
}

pub async fn send_instructions(
    rpc: &RpcClient,
    signer: &Keypair,
    instructions: Vec<Instruction>,
) -> Result<Signature> {
    let recent_blockhash = rpc
        .get_latest_blockhash()
        .await
        .context("latest blockhash")?;
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&signer.pubkey()),
        &[signer],
        recent_blockhash,
    );
    rpc.send_and_confirm_transaction(&tx)
        .await
        .context("sending transaction")
}

pub async fn simulate_instruction(
    rpc: &RpcClient,
    payer: &Keypair,
    instruction: Instruction,
) -> Result<solana_rpc_client_api::response::RpcSimulateTransactionResult> {
    let recent_blockhash = rpc
        .get_latest_blockhash()
        .await
        .context("latest blockhash")?;
    // Product pricing paths (preview_quote) routinely exceed the 200k default.
    // Bump to the 1.4M per-ix max so simulations don't false-fail on CU.
    let cu_limit = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    let mut instructions = vec![cu_limit];
    if let Some(heap_frame_bytes) = simulate_heap_frame_bytes()? {
        instructions.push(ComputeBudgetInstruction::request_heap_frame(
            heap_frame_bytes,
        ));
    }
    instructions.push(instruction);
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    let response = rpc
        .simulate_transaction_with_config(
            &tx,
            RpcSimulateTransactionConfig {
                sig_verify: false,
                replace_recent_blockhash: true,
                commitment: Some(CommitmentConfig::confirmed()),
                ..Default::default()
            },
        )
        .await
        .context("simulating transaction")?;
    if let Some(err) = response.value.err.clone() {
        let logs = response
            .value
            .logs
            .as_ref()
            .map(|l| l.join("\n"))
            .unwrap_or_default();
        return Err(anyhow!("simulation failed: {err:?}\nlogs:\n{logs}"));
    }
    Ok(response.value)
}

pub async fn send_versioned_instructions(
    rpc: &RpcClient,
    signer: &Keypair,
    instructions: Vec<Instruction>,
    address_lookup_table_accounts: Vec<AddressLookupTableAccount>,
) -> Result<Signature> {
    let recent_blockhash = rpc
        .get_latest_blockhash()
        .await
        .context("latest blockhash")?;
    let message = MessageV0::try_compile(
        &signer.pubkey(),
        &instructions,
        &address_lookup_table_accounts,
        recent_blockhash,
    )
    .context("compiling versioned transaction message")?;
    let tx = VersionedTransaction::try_new(VersionedMessage::V0(message), &[signer])
        .context("signing versioned transaction")?;
    rpc.send_and_confirm_transaction(&tx)
        .await
        .context("sending versioned transaction")
}

pub fn decode_return_data<T>(
    result: solana_rpc_client_api::response::RpcSimulateTransactionResult,
    expected_program_id: &solana_sdk::pubkey::Pubkey,
) -> Result<T>
where
    T: anchor_lang::AnchorDeserialize,
{
    let return_data = result
        .return_data
        .ok_or_else(|| anyhow!("simulation returned no Anchor return data"))?;
    if return_data.program_id != expected_program_id.to_string() {
        return Err(anyhow!(
            "unexpected return-data program: expected {}, got {}",
            expected_program_id,
            return_data.program_id
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(return_data.data.0)
        .context("base64-decoding return data")?;
    T::deserialize(&mut bytes.as_slice()).context("decoding Anchor return data")
}
