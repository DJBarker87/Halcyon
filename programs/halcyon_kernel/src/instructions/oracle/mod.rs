pub mod update_ewma;
pub mod write_aggregate_delta;
pub mod write_regime_signal;
pub mod write_regression;

// L-7 — `#[allow(...)]` because Anchor's `#[program]` macro needs the
// per-handler `__client_accounts_*` items reachable via glob. The `handler`
// ambiguity is benign.
#[allow(ambiguous_glob_reexports)]
pub use update_ewma::*;
#[allow(ambiguous_glob_reexports)]
pub use write_aggregate_delta::*;
#[allow(ambiguous_glob_reexports)]
pub use write_regime_signal::*;
#[allow(ambiguous_glob_reexports)]
pub use write_regression::*;
