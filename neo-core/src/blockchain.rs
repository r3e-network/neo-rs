// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use neo_base::errors;


pub struct BlockChain {
    //
}


impl BlockChain {
    pub fn pool_tx(&self) -> Result<(), PoolTxError> {
        Ok(())
    }
}


#[derive(Debug, Clone, errors::Error)]
pub enum PoolTxError {
    //
}