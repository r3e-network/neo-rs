// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use crate::types::{H160, H256};


#[derive(Debug, Clone)]
pub struct CurrentStates {
    pub block_hash: H256,
    pub block_index: u32,
}

pub trait ChainStates {
    // type Error;

    fn current_states(&self) -> CurrentStates; // Result<CurrentStates, Self::Error>;

    fn contains_tx(&self, tx: &H256) -> bool; // Result<bool, Self::Error>;

    fn contains_conflict(&self, tx: &H256, account: &H160) -> bool; // Result<bool, Self::Error>;
}

pub struct BlockChain {
    //
}