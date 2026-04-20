use anchor_lang::{AccountDeserialize, Discriminator};
use anyhow::{Context, Result};
use solana_account_decoder::UiAccountEncoding;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client_api::config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use solana_rpc_client_api::filter::{Memcmp, RpcFilterType};
use solana_sdk::{account::Account, commitment_config::CommitmentConfig, pubkey::Pubkey};

pub fn decode_anchor_account<T>(data: &[u8]) -> Result<T>
where
    T: AccountDeserialize,
{
    let mut slice = data;
    T::try_deserialize(&mut slice).context("decoding Anchor account")
}

pub async fn fetch_anchor_account<T>(rpc: &RpcClient, address: &Pubkey) -> Result<T>
where
    T: AccountDeserialize,
{
    let account = rpc
        .get_account(address)
        .await
        .with_context(|| format!("fetching account {address}"))?;
    decode_anchor_account(&account.data)
}

pub async fn fetch_anchor_account_opt<T>(rpc: &RpcClient, address: &Pubkey) -> Result<Option<T>>
where
    T: AccountDeserialize,
{
    let response = rpc
        .get_account_with_commitment(address, CommitmentConfig::confirmed())
        .await
        .with_context(|| format!("fetching account {address}"))?;
    response
        .value
        .map(|account| decode_anchor_account(&account.data))
        .transpose()
}

pub async fn fetch_multiple_accounts(
    rpc: &RpcClient,
    addresses: &[Pubkey],
) -> Result<Vec<Option<Account>>> {
    rpc.get_multiple_accounts(addresses)
        .await
        .context("fetching multiple accounts")
}

pub async fn list_policy_headers_for_product(
    rpc: &RpcClient,
    product_program_id: &Pubkey,
) -> Result<Vec<(Pubkey, halcyon_kernel::state::PolicyHeader)>> {
    // L-5 — `PolicyHeader` layout: [discriminator (8)][version u8][product_program_id Pubkey][…].
    // Byte offset of `product_program_id` is therefore 8 + 1 = 9. This
    // assertion makes the memcmp offset a compile-time fact so any future
    // field insertion before `product_program_id` fails here rather than
    // silently returning an empty account list (which would cause the
    // hedge keeper to operate on a stale — empty — policy set).
    const PRODUCT_PROGRAM_ID_OFFSET: usize = 8 + 1;
    const _: () = assert!(
        PRODUCT_PROGRAM_ID_OFFSET == 9,
        "PolicyHeader product_program_id offset must stay at 9"
    );
    let filters = vec![
        RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            0,
            halcyon_kernel::state::PolicyHeader::DISCRIMINATOR.to_vec(),
        )),
        RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            PRODUCT_PROGRAM_ID_OFFSET,
            product_program_id.to_bytes().to_vec(),
        )),
    ];
    let config = RpcProgramAccountsConfig {
        filters: Some(filters),
        account_config: RpcAccountInfoConfig {
            encoding: Some(UiAccountEncoding::Base64),
            commitment: Some(CommitmentConfig::confirmed()),
            ..Default::default()
        },
        ..Default::default()
    };
    let accounts = rpc
        .get_program_accounts_with_config(&halcyon_kernel::ID, config)
        .await
        .context("listing policy headers")?;
    accounts
        .into_iter()
        .map(|(address, account)| {
            decode_anchor_account::<halcyon_kernel::state::PolicyHeader>(&account.data)
                .map(|header| (address, header))
        })
        .collect()
}
