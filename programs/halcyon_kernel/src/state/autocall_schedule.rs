use anchor_lang::prelude::*;

pub const AUTOCALL_OBSERVATION_COUNT: usize = 6;

#[account]
#[derive(InitSpace)]
pub struct AutocallSchedule {
    pub version: u8,
    pub product_program_id: Pubkey,
    /// Keeper-supplied reference timestamp used to generate this schedule.
    pub issue_date_ts: i64,
    pub observation_timestamps: [i64; AUTOCALL_OBSERVATION_COUNT],
    pub last_publish_ts: i64,
    pub last_publish_slot: u64,
}

impl AutocallSchedule {
    pub const CURRENT_VERSION: u8 = 1;
}
