// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use neo_base::math::U256;
use crate::types::{H160, H256};


#[derive(Debug, Clone)]
pub struct CurrentStates {
    pub block_index: u32,
    pub block_hash: H256,
}

pub trait BlockStates {
    #[inline]
    fn current_block_index(&self) -> u32 {
        self.current_states().block_index
    }

    #[inline]
    fn current_block_hash(&self) -> H256 {
        self.current_states().block_hash
    }

    fn current_states(&self) -> CurrentStates; // Result<CurrentStates, Self::Error>;
}

pub trait ChainStates: BlockStates {
    // type Error;

    fn contains_tx(&self, tx: &H256) -> bool; // Result<bool, Self::Error>;

    fn contains_conflict(&self, tx: &H256, account: &H160) -> bool; // Result<bool, Self::Error>;
}


pub trait FeeStates {
    fn netfee_per_byte(&self) -> u64;

    fn balance_of(&self, account: &H160) -> U256;
}