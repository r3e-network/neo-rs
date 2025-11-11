use alloc::vec::Vec;

use neo_store::ColumnId;

use super::ExecutionContext;
use crate::error::ContractError;

impl<'a> ExecutionContext<'a> {
    pub fn load(&self, column: ColumnId, key: &[u8]) -> Result<Option<Vec<u8>>, ContractError> {
        self.store
            .get(column, key)
            .map_err(|_| ContractError::Storage("get failed"))
    }

    pub fn put(
        &mut self,
        column: ColumnId,
        key: Vec<u8>,
        value: Vec<u8>,
    ) -> Result<(), ContractError> {
        self.store
            .put(column, key, value)
            .map_err(|_| ContractError::Storage("put failed"))
    }

    pub fn delete(&mut self, column: ColumnId, key: &[u8]) -> Result<(), ContractError> {
        self.store
            .delete(column, key)
            .map_err(|_| ContractError::Storage("delete failed"))
    }
}
