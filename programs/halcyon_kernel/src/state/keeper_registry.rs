use anchor_lang::prelude::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum KeeperRole {
    Observation = 0,
    Regression = 1,
    Delta = 2,
    Hedge = 3,
    Regime = 4,
}

#[account]
#[derive(InitSpace)]
pub struct KeeperRegistry {
    pub version: u8,
    pub observation: Pubkey,
    pub regression: Pubkey,
    pub delta: Pubkey,
    pub hedge: Pubkey,
    pub regime: Pubkey,
    pub last_rotation_ts: i64,
}

impl KeeperRegistry {
    pub const CURRENT_VERSION: u8 = 1;

    pub fn authority_for_role(&self, role: KeeperRole) -> Pubkey {
        match role {
            KeeperRole::Observation => self.observation,
            KeeperRole::Regression => self.regression,
            KeeperRole::Delta => self.delta,
            KeeperRole::Hedge => self.hedge,
            KeeperRole::Regime => self.regime,
        }
    }
}
