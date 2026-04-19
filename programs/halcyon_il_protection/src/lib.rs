//! IL Protection — L0 scaffold. Handlers land in L3.
//!
//! When the issuance path lands, it needs the same preview-to-accept buyer
//! bounds as SOL Autocall: bind the buyer's economic floor and the preview slot
//! freshness instead of trusting a recomputed live quote with those fields
//! unbounded.

use anchor_lang::prelude::*;

declare_id!("HuUQUngf79HgTWdggxAsE135qFeHfYV9Mj9xsCcwqz5g");

#[program]
pub mod halcyon_il_protection {
    use super::*;
}
