pub mod initialize_protocol;
pub mod migrate_protocol_config;
pub mod pause_issuance;
pub mod pause_settlement;
pub mod register_lookup_table;
pub mod register_product;
pub mod rotate_keeper;
pub mod set_protocol_config;
pub mod update_lookup_table;
pub mod update_product_registry;

// L-7 — see `lifecycle/mod.rs` for context. Anchor's program macro needs
// the `__client_accounts_*` siblings reachable via glob.
#[allow(ambiguous_glob_reexports, unused_imports)]
pub use initialize_protocol::*;
#[allow(ambiguous_glob_reexports, unused_imports)]
pub use migrate_protocol_config::*;
#[allow(ambiguous_glob_reexports, unused_imports)]
pub use pause_issuance::*;
#[allow(ambiguous_glob_reexports, unused_imports)]
pub use pause_settlement::*;
#[allow(ambiguous_glob_reexports, unused_imports)]
pub use register_lookup_table::*;
#[allow(ambiguous_glob_reexports, unused_imports)]
pub use register_product::*;
#[allow(ambiguous_glob_reexports, unused_imports)]
pub use rotate_keeper::*;
#[allow(ambiguous_glob_reexports, unused_imports)]
pub use set_protocol_config::*;
#[allow(ambiguous_glob_reexports, unused_imports)]
pub use update_lookup_table::*;
#[allow(ambiguous_glob_reexports, unused_imports)]
pub use update_product_registry::*;
