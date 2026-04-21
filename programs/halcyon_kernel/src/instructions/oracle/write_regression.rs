use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};

use crate::{state::*, KernelError};

const MIN_REGRESSION_SAMPLE_COUNT: u32 = 60;
const MIN_REGRESSION_BETA_S12: i128 = -2_000_000_000_000;
const MAX_REGRESSION_BETA_S12: i128 = 3_000_000_000_000;
const MAX_ABS_REGRESSION_ALPHA_S12: i128 = 5_000_000_000;
const MAX_REGRESSION_R_SQUARED_S6: i64 = 1_000_000;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct WriteRegressionArgs {
    pub beta_spy_s12: i128,
    pub beta_qqq_s12: i128,
    pub alpha_s12: i128,
    pub r_squared_s6: i64,
    pub residual_vol_s6: i64,
    pub window_start_ts: i64,
    pub window_end_ts: i64,
    pub sample_count: u32,
}

#[derive(Accounts)]
pub struct WriteRegression<'info> {
    pub keeper: Signer<'info>,

    #[account(seeds = [seeds::PROTOCOL_CONFIG], bump)]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(seeds = [seeds::KEEPER_REGISTRY], bump)]
    pub keeper_registry: Account<'info, KeeperRegistry>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + Regression::INIT_SPACE,
        seeds = [seeds::REGRESSION],
        bump,
    )]
    pub regression: Account<'info, Regression>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<WriteRegression>, args: WriteRegressionArgs) -> Result<()> {
    require_keys_eq!(
        ctx.accounts.keeper.key(),
        ctx.accounts.keeper_registry.regression,
        HalcyonError::KeeperAuthorityMismatch
    );
    validate_regression_args(&args)?;

    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    let regression = &mut ctx.accounts.regression;

    // K10a — keeper-supplied window must itself be coherent.
    require!(
        args.window_end_ts > args.window_start_ts,
        HalcyonError::OracleTimestampNotMonotonic
    );

    if regression.version == 0 {
        regression.version = Regression::CURRENT_VERSION;
    } else {
        // K10b — strict monotonicity. A compromised keeper cannot backdate
        // a write and DoS staleness checks downstream. New window must
        // strictly advance the previous window_end_ts.
        require!(
            args.window_end_ts > regression.window_end_ts,
            HalcyonError::OracleTimestampNotMonotonic
        );
        // K10c — per-cadence rate limit. Regression keeper is daily; the
        // cap here is 1/3 of `regression_staleness_cap_secs` which gives
        // headroom but rejects bursty writes.
        let min_gap = ctx
            .accounts
            .protocol_config
            .regression_staleness_cap_secs
            .saturating_div(3);
        require!(
            now.saturating_sub(regression.last_update_ts) >= min_gap,
            HalcyonError::OracleRateLimited
        );
    }

    regression.beta_spy_s12 = args.beta_spy_s12;
    regression.beta_qqq_s12 = args.beta_qqq_s12;
    regression.alpha_s12 = args.alpha_s12;
    regression.r_squared_s6 = args.r_squared_s6;
    regression.residual_vol_s6 = args.residual_vol_s6;
    regression.window_start_ts = args.window_start_ts;
    regression.window_end_ts = args.window_end_ts;
    regression.sample_count = args.sample_count;
    regression.last_update_slot = clock.slot;
    regression.last_update_ts = now;
    Ok(())
}

fn validate_regression_args(args: &WriteRegressionArgs) -> Result<()> {
    require!(
        (MIN_REGRESSION_BETA_S12..=MAX_REGRESSION_BETA_S12).contains(&args.beta_spy_s12),
        KernelError::RegressionBetaOutOfRange
    );
    require!(
        (MIN_REGRESSION_BETA_S12..=MAX_REGRESSION_BETA_S12).contains(&args.beta_qqq_s12),
        KernelError::RegressionBetaOutOfRange
    );
    require!(
        (-MAX_ABS_REGRESSION_ALPHA_S12..=MAX_ABS_REGRESSION_ALPHA_S12).contains(&args.alpha_s12),
        KernelError::RegressionAlphaOutOfRange
    );
    require!(
        (0..=MAX_REGRESSION_R_SQUARED_S6).contains(&args.r_squared_s6),
        KernelError::RegressionRSquaredOutOfRange
    );
    require!(
        args.residual_vol_s6 >= 0,
        KernelError::RegressionResidualVolOutOfRange
    );
    require!(
        args.sample_count >= MIN_REGRESSION_SAMPLE_COUNT,
        KernelError::RegressionSampleCountTooLow
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_args() -> WriteRegressionArgs {
        WriteRegressionArgs {
            beta_spy_s12: 900_000_000_000,
            beta_qqq_s12: 400_000_000_000,
            alpha_s12: 50_000_000,
            r_squared_s6: 650_000,
            residual_vol_s6: 220_000,
            window_start_ts: 1,
            window_end_ts: 2,
            sample_count: 90,
        }
    }

    #[test]
    fn rejects_beta_out_of_range() {
        let mut args = base_args();
        args.beta_spy_s12 = MAX_REGRESSION_BETA_S12 + 1;
        let err = validate_regression_args(&args).unwrap_err();
        assert!(
            format!("{err:?}").contains("RegressionBetaOutOfRange"),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn rejects_alpha_out_of_range() {
        let mut args = base_args();
        args.alpha_s12 = MAX_ABS_REGRESSION_ALPHA_S12 + 1;
        let err = validate_regression_args(&args).unwrap_err();
        assert!(
            format!("{err:?}").contains("RegressionAlphaOutOfRange"),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn rejects_r_squared_above_one() {
        let mut args = base_args();
        args.r_squared_s6 = MAX_REGRESSION_R_SQUARED_S6 + 1;
        let err = validate_regression_args(&args).unwrap_err();
        assert!(
            format!("{err:?}").contains("RegressionRSquaredOutOfRange"),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn rejects_negative_residual_vol() {
        let mut args = base_args();
        args.residual_vol_s6 = -1;
        let err = validate_regression_args(&args).unwrap_err();
        assert!(
            format!("{err:?}").contains("RegressionResidualVolOutOfRange"),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn rejects_too_few_samples() {
        let mut args = base_args();
        args.sample_count = MIN_REGRESSION_SAMPLE_COUNT - 1;
        let err = validate_regression_args(&args).unwrap_err();
        assert!(
            format!("{err:?}").contains("RegressionSampleCountTooLow"),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn accepts_plausible_regression_snapshot() {
        validate_regression_args(&base_args()).unwrap();
    }
}
