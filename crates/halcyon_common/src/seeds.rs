//! PDA seed byte-literals. Every kernel and product program derives PDAs from
//! these constants — duplicating the literals in program code is the fastest
//! way to ship a bug that only surfaces when two programs disagree on the
//! derivation.
//!
//! Seeds are referenced by `integration_architecture.md` §2.3 (state topology)
//! and §2.10 (mutual-CPI pattern).

pub const PROTOCOL_CONFIG: &[u8] = b"protocol_config";
pub const PRODUCT_REGISTRY: &[u8] = b"product_registry";
pub const VAULT_STATE: &[u8] = b"vault_state";
pub const SENIOR: &[u8] = b"senior";
pub const JUNIOR: &[u8] = b"junior";
pub const POLICY: &[u8] = b"policy";
pub const TERMS: &[u8] = b"terms";
pub const COUPON_VAULT: &[u8] = b"coupon_vault";
pub const HEDGE_SLEEVE: &[u8] = b"hedge_sleeve";
pub const HEDGE_BOOK: &[u8] = b"hedge_book";
pub const AGGREGATE_DELTA: &[u8] = b"aggregate_delta";
pub const REGRESSION: &[u8] = b"regression";
pub const VAULT_SIGMA: &[u8] = b"vault_sigma";
pub const REGIME_SIGNAL: &[u8] = b"regime_signal";
pub const FEE_LEDGER: &[u8] = b"fee_ledger";
pub const KEEPER_REGISTRY: &[u8] = b"keeper_registry";
pub const ALT_REGISTRY: &[u8] = b"alt_registry";
pub const PRODUCT_AUTHORITY: &[u8] = b"product_authority";
pub const VAULT_AUTHORITY: &[u8] = b"vault_authority";
pub const VAULT_USDC: &[u8] = b"vault_usdc";
pub const TREASURY_USDC: &[u8] = b"treasury_usdc";
