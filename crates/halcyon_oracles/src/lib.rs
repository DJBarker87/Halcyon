//! Halcyon oracle read surface per seam 3.1 of integration_architecture.md.
//!
//! Products never handle raw Pyth bytes. They call `read_pyth_price` with an
//! opaque `AccountInfo`, a feed id (SOL/USD, SPY/USD, …), the expected owner
//! for the selected backend, the current clock, and a per-call staleness cap.
//! The crate handles owner-validation, discriminator-validation, max-age
//! enforcement, and the exponent → SCALE_6 conversion exactly once; the
//! output is a `PriceSnapshot` the product can pass straight into the pricer.
//!
//! Two compile-time backends:
//! - `pyth-pull` (default): reads Pyth's `PriceUpdateV2` account via
//!   `pyth-solana-receiver-sdk`. Production path on mainnet and devnet when
//!   the Pyth receiver is available.
//! - `mock-pyth`: reads a borsh-encoded `MockPriceAccount` owned by the
//!   calling product program. Used by localnet tests and the devnet Pyth
//!   contingency (`research/devnet_mocks/pyth-mock/`) when the real receiver
//!   is unavailable.
//!
//! Exactly one backend is selected at build time. Enabling both is a build
//! error so tests can't accidentally ship with mock pricing.

use anchor_lang::prelude::*;

pub mod errors;
pub mod snapshot;

#[cfg(feature = "pyth-pull")]
pub mod pyth;

#[cfg(feature = "mock-pyth")]
pub mod mock;

pub use errors::OracleError;
pub use snapshot::PriceSnapshot;

#[cfg(all(feature = "pyth-pull", feature = "mock-pyth"))]
compile_error!(
    "halcyon_oracles: enable exactly one of `pyth-pull` or `mock-pyth`. \
     Both are incompatible in a single build — tests that need mock pricing \
     must disable default-features on this crate."
);

#[cfg(not(any(feature = "pyth-pull", feature = "mock-pyth")))]
compile_error!(
    "halcyon_oracles: no oracle backend selected. Enable either `pyth-pull` \
     (default) or `mock-pyth`."
);

/// Read a price feed and produce a normalised [`PriceSnapshot`] at SCALE_6.
///
/// The backend resolves at compile time based on crate features:
/// - `pyth-pull` → [`pyth::read_pyth_price`]
/// - `mock-pyth` → [`mock::read_pyth_price`]
///
/// `staleness_cap_secs` is per-call: products pass
/// `protocol_config.pyth_quote_staleness_cap_secs` at quote time and
/// `protocol_config.pyth_settle_staleness_cap_secs` at settlement/observation.
///
/// `feed_id` is the 32-byte Pyth feed id for the asset being priced. The
/// caller supplies it as a constant (see `halcyon_oracles::feed_ids`). Passing
/// a mismatched id yields `OracleError::FeedIdMismatch` even if the
/// underlying account is well-formed.
#[inline]
pub fn read_pyth_price(
    feed_account: &AccountInfo,
    feed_id: &[u8; 32],
    expected_owner: &Pubkey,
    clock: &Clock,
    staleness_cap_secs: i64,
) -> Result<PriceSnapshot> {
    #[cfg(feature = "pyth-pull")]
    {
        pyth::read_pyth_price(
            feed_account,
            feed_id,
            expected_owner,
            clock,
            staleness_cap_secs,
        )
    }
    #[cfg(feature = "mock-pyth")]
    {
        mock::read_pyth_price(
            feed_account,
            feed_id,
            expected_owner,
            clock,
            staleness_cap_secs,
        )
    }
}

/// Canonical Pyth feed ids used by Halcyon products. Mainnet feed ids per
/// <https://www.pyth.network/developers/price-feed-ids>; devnet uses the same
/// feed id under the devnet Pyth receiver.
pub mod feed_ids {
    /// SOL / USD.
    pub const SOL_USD: [u8; 32] =
        hex_literal("ef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d");

    const fn hex_literal(input: &str) -> [u8; 32] {
        let bytes = input.as_bytes();
        assert!(bytes.len() == 64, "feed id hex must be 64 chars");
        let mut out = [0u8; 32];
        let mut i = 0;
        while i < 32 {
            out[i] = (from_hex(bytes[2 * i]) << 4) | from_hex(bytes[2 * i + 1]);
            i += 1;
        }
        out
    }

    const fn from_hex(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => panic!("non-hex character in feed id"),
        }
    }
}
