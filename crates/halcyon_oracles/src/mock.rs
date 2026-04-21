//! `mock-pyth` backend: reads a simple borsh-encoded account. Used by
//! localnet tests and by the `research/devnet_mocks/pyth-mock/` contingency
//! documented in build_order_part4.md §4.10.
//!
//! The mock account layout:
//! ```text
//! [0..8]   : discriminator = HALCYON_MOCK_PYTH_DISCRIMINATOR
//! [8..40]  : feed_id  ([u8; 32])
//! [40..48] : price   (i64, SCALE_6)
//! [48..56] : conf    (i64, SCALE_6)
//! [56..60] : expo    (i32, informational - kept -6 for the mock)
//! [60..68] : publish_ts (i64)
//! [68..76] : publish_slot (u64)
//! ```
//!
//! The mock is owned by the calling product program so tests can mint and
//! rotate fixture accounts without standing up a Pyth receiver instance.

use anchor_lang::prelude::*;

use crate::errors::OracleError;
use crate::snapshot::PriceSnapshot;

pub const HALCYON_MOCK_PYTH_DISCRIMINATOR: [u8; 8] = *b"HMOCKPYT";

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug)]
pub struct MockPriceAccount {
    pub feed_id: [u8; 32],
    pub price_s6: i64,
    pub conf_s6: i64,
    pub expo: i32,
    pub publish_ts: i64,
    pub publish_slot: u64,
}

impl MockPriceAccount {
    pub const SIZE: usize = 8 + 32 + 8 + 8 + 4 + 8 + 8;
}

pub fn read_pyth_price(
    feed_account: &AccountInfo,
    feed_id: &[u8; 32],
    expected_owner: &Pubkey,
    clock: &Clock,
    staleness_cap_secs: i64,
) -> Result<PriceSnapshot> {
    require_keys_eq!(
        *feed_account.owner,
        *expected_owner,
        OracleError::InvalidOwner
    );

    let data = feed_account.try_borrow_data()?;
    require!(
        data.len() >= MockPriceAccount::SIZE,
        OracleError::MockDiscriminatorMismatch
    );
    require!(
        data[..8] == HALCYON_MOCK_PYTH_DISCRIMINATOR,
        OracleError::MockDiscriminatorMismatch
    );
    let mut slice = &data[8..];
    let acct = MockPriceAccount::deserialize(&mut slice)?;
    require!(acct.feed_id == *feed_id, OracleError::FeedIdMismatch);

    let max_age = acct
        .publish_ts
        .checked_add(staleness_cap_secs)
        .ok_or_else(|| error!(OracleError::ScaleOverflow))?;
    require!(
        max_age >= clock.unix_timestamp,
        OracleError::InsufficientVerification
    );

    Ok(PriceSnapshot {
        price_s6: acct.price_s6,
        conf_s6: acct.conf_s6,
        publish_slot: acct.publish_slot,
        publish_ts: acct.publish_ts,
        expo: acct.expo,
    })
}

pub fn read_pyth_price_in_range(
    feed_account: &AccountInfo,
    feed_id: &[u8; 32],
    expected_owner: &Pubkey,
    min_publish_ts: i64,
    max_publish_ts: i64,
) -> Result<PriceSnapshot> {
    require!(
        min_publish_ts <= max_publish_ts,
        OracleError::PublishTimeOutsideRange
    );
    require_keys_eq!(
        *feed_account.owner,
        *expected_owner,
        OracleError::InvalidOwner
    );

    let data = feed_account.try_borrow_data()?;
    require!(
        data.len() >= MockPriceAccount::SIZE,
        OracleError::MockDiscriminatorMismatch
    );
    require!(
        data[..8] == HALCYON_MOCK_PYTH_DISCRIMINATOR,
        OracleError::MockDiscriminatorMismatch
    );
    let mut slice = &data[8..];
    let acct = MockPriceAccount::deserialize(&mut slice)?;
    require!(acct.feed_id == *feed_id, OracleError::FeedIdMismatch);
    require!(
        (min_publish_ts..=max_publish_ts).contains(&acct.publish_ts),
        OracleError::PublishTimeOutsideRange
    );

    Ok(PriceSnapshot {
        price_s6: acct.price_s6,
        conf_s6: acct.conf_s6,
        publish_slot: acct.publish_slot,
        publish_ts: acct.publish_ts,
        expo: acct.expo,
    })
}

/// Encode a `MockPriceAccount` into its on-chain byte representation. Used by
/// fixture generators and the CLI's `mock-pyth-write` helper.
pub fn encode(acct: &MockPriceAccount) -> Vec<u8> {
    let mut out = Vec::with_capacity(MockPriceAccount::SIZE);
    out.extend_from_slice(&HALCYON_MOCK_PYTH_DISCRIMINATOR);
    acct.serialize(&mut out)
        .expect("borsh vec-write cannot fail");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let acct = MockPriceAccount {
            feed_id: [0xab; 32],
            price_s6: 135_123_456,
            conf_s6: 120_000,
            expo: -6,
            publish_ts: 1_700_000_000,
            publish_slot: 12_345,
        };
        let encoded = encode(&acct);
        assert_eq!(encoded.len(), MockPriceAccount::SIZE);
        assert_eq!(&encoded[..8], &HALCYON_MOCK_PYTH_DISCRIMINATOR);
        let mut slice = &encoded[8..];
        let round = MockPriceAccount::deserialize(&mut slice).unwrap();
        assert_eq!(round.price_s6, 135_123_456);
        assert_eq!(round.feed_id, [0xab; 32]);
    }

    #[test]
    fn rejects_unexpected_owner() {
        let feed_id = [0xcd; 32];
        let acct = MockPriceAccount {
            feed_id,
            price_s6: 150_000_000,
            conf_s6: 100_000,
            expo: -6,
            publish_ts: 1_700_000_000,
            publish_slot: 99,
        };
        let key = Pubkey::new_unique();
        let actual_owner = Pubkey::new_unique();
        let expected_owner = Pubkey::new_unique();
        let mut lamports = 0u64;
        let mut data = encode(&acct);
        let info = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            &mut data,
            &actual_owner,
            false,
            0,
        );
        let clock = Clock {
            slot: 123,
            epoch_start_timestamp: 0,
            epoch: 0,
            leader_schedule_epoch: 0,
            unix_timestamp: acct.publish_ts,
        };

        let err = read_pyth_price(&info, &feed_id, &expected_owner, &clock, 10).unwrap_err();
        assert_eq!(err, error!(OracleError::InvalidOwner));
    }

    #[test]
    fn rejects_publish_time_outside_range() {
        let feed_id = [0xee; 32];
        let acct = MockPriceAccount {
            feed_id,
            price_s6: 123_000_000,
            conf_s6: 1_000,
            expo: -6,
            publish_ts: 1_700_000_000,
            publish_slot: 77,
        };
        let key = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let mut lamports = 0u64;
        let mut data = encode(&acct);
        let info = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            &mut data,
            &owner,
            false,
            0,
        );

        let err = read_pyth_price_in_range(
            &info,
            &feed_id,
            &owner,
            acct.publish_ts + 1,
            acct.publish_ts + 5,
        )
        .unwrap_err();
        assert_eq!(err, error!(OracleError::PublishTimeOutsideRange));
    }
}
