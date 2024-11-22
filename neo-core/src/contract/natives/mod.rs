// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::sync::Arc;

use crate::store::NeoStates;

pub use {gas::*, neo::*, oracle::*, policy::*};

pub mod gas;
pub mod neo;
pub mod oracle;
pub mod policy;

// const GAS_FACTOR: u64 = 100_000_000;
// const DEFAULT_REGISTER_PRICE: u64 = 1000 * GAS_FACTOR;
// const NEO_HOLDER_REWARD_RATIO: u64 = 10;
// const COMMITTEE_REWARD_RATIO: u64 = 10;
// const VOTER_REWARD_RATIO: u64 = 80;

pub struct NativeContracts {
    pub states: Arc<dyn NeoStates>,
    pub neo: NeoContract,
    pub gas: GasContract,
    pub policy: PolicyContract,
    pub oracle: OracleContract,
}

impl NativeContracts {
    pub fn new(states: Arc<dyn NeoStates>) -> Self {
        Self {
            states,
            neo: NeoContract {},
            gas: GasContract {},
            policy: PolicyContract {},
            oracle: OracleContract {},
        }
    }
}
