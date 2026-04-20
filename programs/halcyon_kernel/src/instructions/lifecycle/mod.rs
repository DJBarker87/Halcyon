pub mod apply_settlement;
pub mod finalize_policy;
pub mod pay_coupon;
pub mod prepare_hedge_swap;
pub mod reap_quoted;
pub mod record_hedge_trade;
pub mod reserve_and_issue;

// L-7 — glob re-export is required so Anchor's `#[program]` macro can reach
// each handler's `__client_accounts_*` / `__cpi_client_accounts_*`
// siblings. The `ambiguous_glob_reexports` warning on `handler` names is
// benign (Rust's resolution is deterministic and the `#[program]` macro
// addresses handlers via fully-qualified paths, not via `crate::handler`).
#[allow(ambiguous_glob_reexports)]
pub use apply_settlement::*;
#[allow(ambiguous_glob_reexports)]
pub use finalize_policy::*;
#[allow(ambiguous_glob_reexports)]
pub use pay_coupon::*;
#[allow(ambiguous_glob_reexports)]
pub use prepare_hedge_swap::*;
#[allow(ambiguous_glob_reexports)]
pub use reap_quoted::*;
#[allow(ambiguous_glob_reexports)]
pub use record_hedge_trade::*;
#[allow(ambiguous_glob_reexports)]
pub use reserve_and_issue::*;
