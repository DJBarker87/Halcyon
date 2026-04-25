use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};

use crate::state::{AutocallSchedule, CouponSchedule, KeeperRegistry, ProductRegistryEntry};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct WriteAutocallScheduleArgs {
    pub product_program_id: Pubkey,
    pub issue_date_ts: i64,
    pub coupon_timestamps: [i64; 18],
    pub observation_timestamps: [i64; 6],
}

#[derive(Accounts)]
#[instruction(args: WriteAutocallScheduleArgs)]
pub struct WriteAutocallSchedule<'info> {
    pub keeper: Signer<'info>,

    #[account(seeds = [seeds::KEEPER_REGISTRY], bump)]
    pub keeper_registry: Account<'info, KeeperRegistry>,

    #[account(
        seeds = [seeds::PRODUCT_REGISTRY, args.product_program_id.as_ref()],
        bump,
        constraint = product_registry_entry.product_program_id == args.product_program_id
            @ crate::KernelError::ProductProgramMismatch,
        constraint = product_registry_entry.active @ HalcyonError::ProductNotRegistered,
    )]
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + AutocallSchedule::INIT_SPACE,
        seeds = [seeds::AUTOCALL_SCHEDULE, args.product_program_id.as_ref()],
        bump,
    )]
    pub autocall_schedule: Account<'info, AutocallSchedule>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + CouponSchedule::INIT_SPACE,
        seeds = [seeds::COUPON_SCHEDULE, args.product_program_id.as_ref()],
        bump,
    )]
    pub coupon_schedule: Account<'info, CouponSchedule>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<WriteAutocallSchedule>, args: WriteAutocallScheduleArgs) -> Result<()> {
    require_keys_eq!(
        ctx.accounts.keeper.key(),
        ctx.accounts.keeper_registry.observation,
        HalcyonError::KeeperAuthorityMismatch
    );
    validate_schedule_args(&args)?;

    let clock = Clock::get()?;
    let schedule = &mut ctx.accounts.autocall_schedule;
    let coupon_schedule = &mut ctx.accounts.coupon_schedule;

    if schedule.version == 0 {
        schedule.version = AutocallSchedule::CURRENT_VERSION;
    } else {
        require_keys_eq!(
            schedule.product_program_id,
            args.product_program_id,
            crate::KernelError::ProductProgramMismatch
        );
        require!(
            args.issue_date_ts > schedule.issue_date_ts,
            HalcyonError::OracleTimestampNotMonotonic
        );
    }
    if coupon_schedule.version == 0 {
        coupon_schedule.version = CouponSchedule::CURRENT_VERSION;
    } else {
        require_keys_eq!(
            coupon_schedule.product_program_id,
            args.product_program_id,
            crate::KernelError::ProductProgramMismatch
        );
        require!(
            args.issue_date_ts > coupon_schedule.issue_date_ts,
            HalcyonError::OracleTimestampNotMonotonic
        );
    }

    schedule.product_program_id = args.product_program_id;
    schedule.issue_date_ts = args.issue_date_ts;
    schedule.observation_timestamps = args.observation_timestamps;
    schedule.last_publish_ts = clock.unix_timestamp;
    schedule.last_publish_slot = clock.slot;

    coupon_schedule.product_program_id = args.product_program_id;
    coupon_schedule.issue_date_ts = args.issue_date_ts;
    coupon_schedule.observation_timestamps = args.coupon_timestamps;
    coupon_schedule.last_publish_ts = clock.unix_timestamp;
    coupon_schedule.last_publish_slot = clock.slot;
    Ok(())
}

fn validate_schedule_args(args: &WriteAutocallScheduleArgs) -> Result<()> {
    require!(
        args.issue_date_ts > 0,
        HalcyonError::OracleTimestampNotMonotonic
    );
    let mut prev = args.issue_date_ts;
    for ts in args.coupon_timestamps {
        require!(ts > prev, HalcyonError::OracleTimestampNotMonotonic);
        prev = ts;
    }
    require!(
        args.observation_timestamps[0] == args.coupon_timestamps[2]
            && args.observation_timestamps[1] == args.coupon_timestamps[5]
            && args.observation_timestamps[2] == args.coupon_timestamps[8]
            && args.observation_timestamps[3] == args.coupon_timestamps[11]
            && args.observation_timestamps[4] == args.coupon_timestamps[14]
            && args.observation_timestamps[5] == args.coupon_timestamps[17],
        HalcyonError::OracleTimestampNotMonotonic
    );
    prev = args.issue_date_ts;
    for ts in args.observation_timestamps {
        require!(ts > prev, HalcyonError::OracleTimestampNotMonotonic);
        prev = ts;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_args() -> WriteAutocallScheduleArgs {
        WriteAutocallScheduleArgs {
            product_program_id: crate::ID,
            issue_date_ts: 1_700_000_000,
            coupon_timestamps: [
                1_700_033_333,
                1_700_066_666,
                1_700_100_000,
                1_700_133_333,
                1_700_166_666,
                1_700_200_000,
                1_700_233_333,
                1_700_266_666,
                1_700_300_000,
                1_700_333_333,
                1_700_366_666,
                1_700_400_000,
                1_700_433_333,
                1_700_466_666,
                1_700_500_000,
                1_700_533_333,
                1_700_566_666,
                1_700_600_000,
            ],
            observation_timestamps: [
                1_700_100_000,
                1_700_200_000,
                1_700_300_000,
                1_700_400_000,
                1_700_500_000,
                1_700_600_000,
            ],
        }
    }

    #[test]
    fn rejects_non_monotonic_observations() {
        let mut args = base_args();
        args.observation_timestamps[3] = args.observation_timestamps[2];
        let err = validate_schedule_args(&args).unwrap_err();
        assert!(
            format!("{err:?}").contains("OracleTimestampNotMonotonic"),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn accepts_strictly_increasing_schedule() {
        validate_schedule_args(&base_args()).unwrap();
    }
}
