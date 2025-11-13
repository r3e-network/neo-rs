use neo_base::{hash::Hash160, Bytes};
use neo_store::ColumnId;

use crate::{error::VmError, value::VmValue};

/// Execution trigger for smart contracts, mirroring Neo's `TriggerType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Application,
    Verification,
}

impl Trigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Application => "Application",
            Trigger::Verification => "Verification",
        }
    }
}

/// Host interface required by the VM to service syscalls.
pub trait RuntimeHost {
    fn log(&mut self, message: String);
    fn notify(&mut self, event: String, payload: Vec<VmValue>) -> Result<(), VmError>;
    fn load_storage(&mut self, column: ColumnId, key: &[u8]) -> Result<Option<Vec<u8>>, VmError>;
    fn put_storage(&mut self, column: ColumnId, key: &[u8], value: &[u8]) -> Result<(), VmError>;
    fn delete_storage(&mut self, column: ColumnId, key: &[u8]) -> Result<(), VmError>;
    fn find_storage_iterator(
        &mut self,
        column: ColumnId,
        prefix: &[u8],
        options: u8,
    ) -> Result<u32, VmError>;
    fn storage_iterator_next(&mut self, handle: u32) -> Result<Option<VmValue>, VmError>;
    fn timestamp(&self) -> i64;
    fn invocation_counter(&self) -> u32;
    fn storage_context_bytes(&self) -> Bytes;
    fn script(&self) -> Bytes;
    fn script_hash(&self) -> Option<Hash160>;
    fn calling_script_hash(&self) -> Option<Hash160>;
    fn entry_script_hash(&self) -> Option<Hash160>;
    fn platform(&self) -> &str;
    fn trigger(&self) -> Trigger;
    fn check_witness(&self, target: &Hash160) -> bool;
}
