use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};

use crate::state::{AutocallSchedule, KeeperRegistry, ProductRegistryEntry};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct WriteAutocallScheduleArgs {
    pub product_program_id: Pubkey,
    pub issue_date_ts: i64,
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

    schedule.product_program_id = args.product_program_id;
    schedule.issue_date_ts = args.issue_date_ts;
    schedule.observation_timestamps = args.observation_timestamps;
    schedule.last_publish_ts = clock.unix_timestamp;
    schedule.last_publish_slot = clock.slot;
    Ok(())
}

fn validate_schedule_args(args: &WriteAutocallScheduleArgs) -> Result<()> {
    require!(
        args.issue_date_ts > 0,
        HalcyonError::OracleTimestampNotMonotonic
    );
    let mut prev = args.issue_date_ts;
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
