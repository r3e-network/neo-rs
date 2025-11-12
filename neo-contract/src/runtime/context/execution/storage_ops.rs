use alloc::vec::Vec;

use neo_store::ColumnId;

use super::ExecutionContext;
use crate::{error::ContractError, nef::CallFlags};

impl<'a> ExecutionContext<'a> {
    pub fn load(&self, column: ColumnId, key: &[u8]) -> Result<Option<Vec<u8>>, ContractError> {
        self.require_call_flag(CallFlags::READ_STATES)?;
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
        self.require_call_flag(CallFlags::WRITE_STATES)?;
        self.store
            .put(column, key, value)
            .map_err(|_| ContractError::Storage("put failed"))
    }

    pub fn delete(&mut self, column: ColumnId, key: &[u8]) -> Result<(), ContractError> {
        self.require_call_flag(CallFlags::WRITE_STATES)?;
        self.store
            .delete(column, key)
            .map_err(|_| ContractError::Storage("delete failed"))
    }

    pub(crate) fn set_call_flags(&mut self, flags: CallFlags) {
        self.current_call_flags = flags;
    }

    pub(crate) fn call_flags(&self) -> CallFlags {
        self.current_call_flags
    }

    pub(crate) fn require_call_flag(&self, required: CallFlags) -> Result<(), ContractError> {
        if required == CallFlags::NONE || self.current_call_flags.contains(required) {
            Ok(())
        } else {
            Err(ContractError::MissingCallFlags(required))
        }
    }
}
