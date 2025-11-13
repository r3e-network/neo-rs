use neo_base::hash::Hash160;
use neo_store::ColumnId;
use neo_vm::{RuntimeHost, Trigger, VmError, VmValue};

use crate::{nef::CallFlags, runtime::value::Value};

use super::ExecutionContext;

impl RuntimeHost for ExecutionContext<'_> {
    fn log(&mut self, message: String) {
        self.push_log(message);
    }

    fn notify(&mut self, event: String, payload: Vec<VmValue>) -> Result<(), VmError> {
        self.require_call_flag(CallFlags::ALLOW_NOTIFY)
            .map_err(|_| VmError::NativeFailure("notify requires AllowNotify"))?;
        let converted = payload.into_iter().map(Value::from).collect();
        self.push_notification(event, converted);
        Ok(())
    }

    fn load_storage(&mut self, column: ColumnId, key: &[u8]) -> Result<Option<Vec<u8>>, VmError> {
        self.load(column, key)
            .map_err(|_| VmError::NativeFailure("storage get"))
    }

    fn put_storage(&mut self, column: ColumnId, key: &[u8], value: &[u8]) -> Result<(), VmError> {
        self.put(column, key.to_vec(), value.to_vec())
            .map_err(|_| VmError::NativeFailure("storage put"))
    }

    fn delete_storage(&mut self, column: ColumnId, key: &[u8]) -> Result<(), VmError> {
        self.delete(column, key)
            .map_err(|_| VmError::NativeFailure("storage delete"))
    }

    fn find_storage_iterator(
        &mut self,
        column: ColumnId,
        prefix: &[u8],
        options: u8,
    ) -> Result<u32, VmError> {
        self.create_storage_iterator_from_bits(column, prefix, options)
            .map_err(|_| VmError::NativeFailure("storage find"))
    }

    fn storage_iterator_next(&mut self, handle: u32) -> Result<Option<VmValue>, VmError> {
        ExecutionContext::storage_iterator_next(self, handle)
            .map_err(|_| VmError::NativeFailure("storage next"))
    }

    fn timestamp(&self) -> i64 {
        self.timestamp()
    }

    fn invocation_counter(&self) -> u32 {
        self.invocation_counter()
    }

    fn storage_context_bytes(&self) -> neo_base::Bytes {
        self.storage_context().to_bytes()
    }

    fn script(&self) -> neo_base::Bytes {
        self.script().clone()
    }

    fn script_hash(&self) -> Option<Hash160> {
        self.current_script_hash()
    }

    fn calling_script_hash(&self) -> Option<Hash160> {
        self.calling_script_hash()
    }

    fn entry_script_hash(&self) -> Option<Hash160> {
        self.entry_script_hash()
    }

    fn platform(&self) -> &str {
        self.platform()
    }

    fn check_witness(&self, hash: &Hash160) -> bool {
        self.check_witness(hash)
    }

    fn trigger(&self) -> Trigger {
        self.trigger()
    }

    fn call_flags(&self) -> u8 {
        self.call_flags().bits()
    }
}
