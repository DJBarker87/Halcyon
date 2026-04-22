//! Flagship worst-of-3 equity autocall — L4 foundation.

use anchor_lang::prelude::*;

pub mod calendar;
pub mod errors;
pub mod instructions;
mod observation;
pub mod pricing;
pub mod state;

pub use errors::FlagshipAutocallError;
#[allow(ambiguous_glob_reexports)]
pub use instructions::accept_quote::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::preview_quote::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::reconcile_coupons::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::record_autocall_observation::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::record_coupon_observation::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::record_ki_event::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::settle::*;
pub use state::*;

declare_id!("E4Atu2kHkzJ1NMATBvoMcy3BDKfsyz418DHCoqQHc3Mc");

#[program]
pub mod halcyon_flagship_autocall {
    use super::*;

    pub fn preview_quote(ctx: Context<PreviewQuote>, notional_usdc: u64) -> Result<QuotePreview> {
        instructions::preview_quote::handler(ctx, notional_usdc)
    }

    pub fn accept_quote(ctx: Context<AcceptQuote>, args: AcceptQuoteArgs) -> Result<()> {
        instructions::accept_quote::handler(ctx, args)
    }

    pub fn record_coupon_observation(
        ctx: Context<RecordCouponObservation>,
        expected_index: u8,
    ) -> Result<()> {
        instructions::record_coupon_observation::handler(ctx, expected_index)
    }

    pub fn reconcile_coupons(ctx: Context<ReconcileCoupons>) -> Result<()> {
        instructions::reconcile_coupons::handler(ctx)
    }

    pub fn record_autocall_observation(
        ctx: Context<RecordAutocallObservation>,
        expected_index: u8,
    ) -> Result<()> {
        instructions::record_autocall_observation::handler(ctx, expected_index)
    }

    pub fn record_ki_event(ctx: Context<RecordKiEvent>) -> Result<()> {
        instructions::record_ki_event::handler(ctx)
    }

    pub fn settle(ctx: Context<Settle>) -> Result<()> {
        instructions::settle::handler(ctx)
    }
}
