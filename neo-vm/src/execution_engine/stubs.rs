//
// Default host hooks for ExecutionEngine runs without an attached ApplicationEngine.
//

use super::{ExecutionEngine, VmResult, HASH_SIZE};

impl ExecutionEngine {
    /// Returns the current script hash, if the host provides one.
    /// Standalone VM execution has no blockchain context so this returns `None`.
    #[must_use]
    pub const fn current_script_hash(&self) -> Option<&[u8]> {
        None
    }

    /// Returns the script container, if the host provides one.
    #[must_use]
    pub fn get_script_container(&self) -> Option<&dyn std::any::Any> {
        None
    }

    /// Gets the script container hash for signature verification.
    /// Returns the hash of the current transaction or block being executed.
    #[must_use]
    pub fn get_script_container_hash(&self) -> Vec<u8> {
        vec![0u8; HASH_SIZE]
    }

    /// Returns the trigger type for this execution when available.
    #[must_use]
    pub const fn get_trigger_type(&self) -> u8 {
        0x40
    }

    /// Emits a runtime log event when a host is attached.
    pub fn emit_runtime_log_event(&mut self, _message: &str) -> VmResult<()> {
        Ok(())
    }

    /// Records an execution log when a host is attached.
    pub fn add_execution_log(&mut self, _message: String) -> VmResult<()> {
        Ok(())
    }

    /// Returns the current transaction hash when executed with blockchain context.
    #[must_use]
    pub const fn get_transaction_hash(&self) -> Option<Vec<u8>> {
        None
    }

    /// Returns the current block hash when executed with blockchain context.
    #[must_use]
    pub const fn get_current_block_hash(&self) -> Option<Vec<u8>> {
        None
    }

    /// Returns a storage item when executed with blockchain context.
    #[must_use]
    pub const fn get_storage_item(&self, _key: &[u8]) -> Option<Vec<u8>> {
        None
    }
}
