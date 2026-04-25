//! `pyth-pull` backend: reads Pyth's `PriceUpdateV2` via
//! `pyth-solana-receiver-sdk` with `VerificationLevel::Full`.

use anchor_lang::prelude::*;
use pyth_solana_receiver_sdk::price_update::{PriceUpdateV2, VerificationLevel};

use crate::errors::OracleError;
use crate::snapshot::PriceSnapshot;

pub fn read_pyth_price(
    feed_account: &AccountInfo,
    feed_id: &[u8; 32],
    _expected_owner: &Pubkey,
    clock: &Clock,
    staleness_cap_secs: i64,
) -> Result<PriceSnapshot> {
    // Owner must be the Pyth receiver. The SDK's account constraint would do
    // this if the product declared Account<PriceUpdateV2>, but the seam
    // signature accepts an opaque AccountInfo so we re-verify here.
    require_keys_eq!(
        *feed_account.owner,
        pyth_solana_receiver_sdk::ID,
        OracleError::InvalidOwner
    );

    // Deserialize; `try_deserialize` validates the 8-byte anchor discriminator.
    let data = feed_account.try_borrow_data()?;
    let mut slice: &[u8] = &data;
    let update = PriceUpdateV2::try_deserialize(&mut slice)?;

    // Feed-id match is a hard error: a SOL account routed into an SPY slot
    // would otherwise produce a nonsense but well-formed PriceSnapshot.
    require!(
        update.price_message.feed_id == *feed_id,
        OracleError::FeedIdMismatch
    );

    // Keep verification and freshness as separate errors so demo/operator UI
    // can distinguish an unverified receiver update from a market-closed feed.
    require!(
        update.verification_level.gte(VerificationLevel::Full),
        OracleError::InsufficientVerification
    );
    let price = update
        .get_price_unchecked(feed_id)
        .map_err(|_| error!(OracleError::FeedIdMismatch))?;
    let max_age = price
        .publish_time
        .checked_add(staleness_cap_secs)
        .ok_or_else(|| error!(OracleError::ScaleOverflow))?;
    require!(max_age >= clock.unix_timestamp, OracleError::PythPriceStale);

    PriceSnapshot::from_raw(
        price.price,
        price.conf,
        price.exponent,
        price.publish_time,
        update.posted_slot,
    )
}

pub fn read_pyth_price_in_range(
    feed_account: &AccountInfo,
    feed_id: &[u8; 32],
    _expected_owner: &Pubkey,
    min_publish_ts: i64,
    max_publish_ts: i64,
) -> Result<PriceSnapshot> {
    require!(
        min_publish_ts <= max_publish_ts,
        OracleError::PublishTimeOutsideRange
    );
    require_keys_eq!(
        *feed_account.owner,
        pyth_solana_receiver_sdk::ID,
        OracleError::InvalidOwner
    );

    let data = feed_account.try_borrow_data()?;
    let mut slice: &[u8] = &data;
    let update = PriceUpdateV2::try_deserialize(&mut slice)?;
    require!(
        update.price_message.feed_id == *feed_id,
        OracleError::FeedIdMismatch
    );
    require!(
        update.verification_level.gte(VerificationLevel::Full),
        OracleError::InsufficientVerification
    );
    let price = update
        .get_price_unchecked(feed_id)
        .map_err(|_| error!(OracleError::FeedIdMismatch))?;
    require!(
        (min_publish_ts..=max_publish_ts).contains(&price.publish_time),
        OracleError::PublishTimeOutsideRange
    );

    PriceSnapshot::from_raw(
        price.price,
        price.conf,
        price.exponent,
        price.publish_time,
        update.posted_slot,
    )
}
