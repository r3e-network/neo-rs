// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use neo_base::errors;

use crate::tx::{Tx, TxPool};

#[allow(unused)]
pub struct BlockChain {
    txpool: TxPool,
}

impl BlockChain {
    pub fn pool_tx(&self, _tx: Tx) -> Result<(), PoolTxError> { Ok(()) }

    pub fn verify_tx(&self, _tx: &Tx) -> Result<(), PoolTxError> { Ok(()) }
}

#[derive(Debug, Clone, errors::Error)]
pub enum PoolTxError {
    //
}
