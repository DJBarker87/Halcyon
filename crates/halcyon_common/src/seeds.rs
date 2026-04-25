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
pub const PENDING_HEDGE_SWAP: &[u8] = b"pending_hedge_swap";
pub const AGGREGATE_DELTA: &[u8] = b"aggregate_delta";
pub const REGRESSION: &[u8] = b"regression";
pub const AUTOCALL_SCHEDULE: &[u8] = b"autocall_schedule";
pub const COUPON_SCHEDULE: &[u8] = b"coupon_schedule";
pub const VAULT_SIGMA: &[u8] = b"vault_sigma";
pub const REGIME_SIGNAL: &[u8] = b"regime_signal";
pub const REDUCED_OPERATORS: &[u8] = b"reduced_operators";
pub const MIDLIFE_MATRICES: &[u8] = b"midlife_matrices";
pub const FEE_LEDGER: &[u8] = b"fee_ledger";
pub const KEEPER_REGISTRY: &[u8] = b"keeper_registry";
pub const ALT_REGISTRY: &[u8] = b"alt_registry";
pub const PRODUCT_AUTHORITY: &[u8] = b"product_authority";
pub const VAULT_AUTHORITY: &[u8] = b"vault_authority";
pub const VAULT_USDC: &[u8] = b"vault_usdc";
pub const TREASURY_USDC: &[u8] = b"treasury_usdc";
pub const POLICY_RECEIPT: &[u8] = b"policy_receipt";
pub const POLICY_RECEIPT_MINT: &[u8] = b"policy_receipt_mint";
pub const POLICY_RECEIPT_AUTHORITY: &[u8] = b"policy_receipt_authority";
pub const RETAIL_REDEMPTION: &[u8] = b"retail_redemption";
