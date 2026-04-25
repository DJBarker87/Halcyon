//! Flagship worst-of-3 equity autocall — L4 foundation.

use anchor_lang::prelude::*;

pub mod buyback_math;
pub mod calendar;
pub mod errors;
pub mod instructions;
pub mod midlife_pricing;
mod observation;
pub mod pricing;
pub mod state;

pub use errors::FlagshipAutocallError;
#[allow(ambiguous_glob_reexports)]
pub use instructions::accept_quote::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::advance_midlife_nav::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::buyback::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::buyback_from_checkpoint::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::cancel_retail_redemption::*;
#[cfg(all(
    any(feature = "integration-test", feature = "idl-build"),
    not(feature = "cpi")
))]
#[allow(ambiguous_glob_reexports)]
pub use instructions::debug_midlife_nav::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::execute_retail_redemption::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::prepare_midlife_nav::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::preview_lending_value::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::preview_lending_value_from_checkpoint::*;
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
pub use instructions::request_retail_redemption::*;
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

    pub fn preview_lending_value(ctx: Context<PreviewLendingValue>) -> Result<LendingValuePreview> {
        instructions::preview_lending_value::handler(ctx)
    }

    pub fn prepare_midlife_nav(
        ctx: Context<PrepareMidlifeNav>,
        stop_coupon_index: u8,
    ) -> Result<MidlifeNavCheckpointPreview> {
        instructions::prepare_midlife_nav::handler(ctx, stop_coupon_index)
    }

    pub fn advance_midlife_nav(
        ctx: Context<AdvanceMidlifeNav>,
        stop_coupon_index: u8,
    ) -> Result<MidlifeNavCheckpointPreview> {
        instructions::advance_midlife_nav::handler(ctx, stop_coupon_index)
    }

    pub fn preview_lending_value_from_checkpoint(
        ctx: Context<PreviewLendingValueFromCheckpoint>,
    ) -> Result<LendingValuePreview> {
        instructions::preview_lending_value_from_checkpoint::handler(ctx)
    }

    pub fn buyback(ctx: Context<Buyback>) -> Result<()> {
        instructions::buyback::handler(ctx)
    }

    pub fn buyback_from_checkpoint(ctx: Context<BuybackFromCheckpoint>) -> Result<()> {
        instructions::buyback_from_checkpoint::handler(ctx)
    }

    pub fn request_retail_redemption(ctx: Context<RequestRetailRedemption>) -> Result<()> {
        instructions::request_retail_redemption::handler(ctx)
    }

    pub fn cancel_retail_redemption(ctx: Context<CancelRetailRedemption>) -> Result<()> {
        instructions::cancel_retail_redemption::handler(ctx)
    }

    pub fn execute_retail_redemption(ctx: Context<ExecuteRetailRedemption>) -> Result<()> {
        instructions::execute_retail_redemption::handler(ctx)
    }

    #[cfg(all(
        any(feature = "integration-test", feature = "idl-build"),
        not(feature = "cpi")
    ))]
    pub fn debug_midlife_nav(
        ctx: Context<DebugMidlifeNavView>,
        inputs: DebugMidlifeInputs,
    ) -> Result<DebugMidlifeNav> {
        instructions::debug_midlife_nav::handler(ctx, inputs)
    }

    #[cfg(all(
        any(feature = "integration-test", feature = "idl-build"),
        not(feature = "cpi")
    ))]
    pub fn debug_midlife_nav_prepare(
        ctx: Context<DebugMidlifeNavPrepare>,
        inputs: DebugMidlifeInputs,
        stop_coupon_index: u8,
    ) -> Result<MidlifeNavCheckpointPreview> {
        instructions::debug_midlife_nav::prepare_handler(ctx, inputs, stop_coupon_index)
    }

    #[cfg(all(
        any(feature = "integration-test", feature = "idl-build"),
        not(feature = "cpi")
    ))]
    pub fn debug_midlife_nav_advance(
        ctx: Context<DebugMidlifeNavAdvance>,
        stop_coupon_index: u8,
    ) -> Result<MidlifeNavCheckpointPreview> {
        instructions::debug_midlife_nav::advance_handler(ctx, stop_coupon_index)
    }

    #[cfg(all(
        any(feature = "integration-test", feature = "idl-build"),
        not(feature = "cpi")
    ))]
    pub fn debug_midlife_nav_finish(
        ctx: Context<DebugMidlifeNavFinish>,
    ) -> Result<DebugMidlifeNav> {
        instructions::debug_midlife_nav::finish_handler(ctx)
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
