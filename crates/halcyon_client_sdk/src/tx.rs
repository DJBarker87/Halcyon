use anyhow::{anyhow, Context, Result};
use base64::Engine;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client_api::config::RpcSimulateTransactionConfig;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::Instruction,
    signature::{Keypair, Signature},
    signer::Signer,
    transaction::Transaction,
};

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
    let tx = Transaction::new_signed_with_payer(
        &[instruction],
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
        return Err(anyhow!("simulation failed: {err:?}"));
    }
    Ok(response.value)
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
