//! Every kernel-owned PDA layout.
//!
//! Fields are documented in `programs/halcyon_kernel/LAYOUTS.md`; the two
//! documents drift at integration cost. `make layouts-check` validates
//! parity against the compiled IDL at layer boundary.

pub mod aggregate_delta;
pub mod coupon_vault;
pub mod fee_ledger;
pub mod hedge_book;
pub mod hedge_sleeve;
pub mod junior;
pub mod keeper_registry;
pub mod lookup_table_registry;
pub mod pending_hedge_swap;
pub mod policy;
pub mod product_registry;
pub mod protocol_config;
pub mod regime_signal;
pub mod regression;
pub mod senior;
pub mod vault_sigma;
pub mod vault_state;

pub use aggregate_delta::AggregateDelta;
pub use coupon_vault::CouponVault;
pub use fee_ledger::FeeLedger;
pub use hedge_book::HedgeBookState;
pub use hedge_sleeve::HedgeSleeve;
pub use junior::JuniorTranche;
pub use keeper_registry::{KeeperRegistry, KeeperRole};
pub use lookup_table_registry::LookupTableRegistry;
pub use pending_hedge_swap::PendingHedgeSwap;
pub use policy::{PolicyHeader, PolicyStatus};
pub use product_registry::ProductRegistryEntry;
pub use protocol_config::ProtocolConfig;
pub use regime_signal::{Regime, RegimeSignal};
pub use regression::Regression;
pub use senior::SeniorDeposit;
pub use vault_sigma::VaultSigma;
pub use vault_state::VaultState;
