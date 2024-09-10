// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use neo_base::errors;
use crate::network::Payloads::Transaction;

#[allow(unused)]
pub struct BlockChain {
    txpool: TxPool,
}


impl BlockChain {
    pub fn pool_tx(&self, _tx: Transaction) -> Result<(), PoolTxError> {
        Ok(())
    }

    pub fn verify_tx(&self, _tx: &Transaction) -> Result<(), PoolTxError> {
        Ok(())
    }
}


#[derive(Debug, Clone, errors::Error)]
pub enum PoolTxError {
    //
}
