//
// Default host hooks for ExecutionEngine runs without an attached ApplicationEngine.
//

use super::*;

impl ExecutionEngine {
    /// Gets gas consumed (disabled - no C# counterpart)
    /// ApplicationEngine overrides this with additional gas tracking
    pub fn gas_consumed(&self) -> i64 {
        0
    }

    /// Gets gas limit (disabled - no C# counterpart)
    /// ApplicationEngine overrides this with actual gas limit
    pub fn gas_limit(&self) -> i64 {
        1_000_000_000 // Default gas limit
    }

    /// Returns the current script hash, if the host provides one.
    /// Standalone VM execution has no blockchain context so this returns `None`.
    pub fn current_script_hash(&self) -> Option<&[u8]> {
        None
    }

    /// Returns the script container, if the host provides one.
    pub fn get_script_container(&self) -> Option<&dyn std::any::Any> {
        None
    }

    /// Gets the script container hash for signature verification.
    /// Returns the hash of the current transaction or block being executed.
    pub fn get_script_container_hash(&self) -> Vec<u8> {
        vec![0u8; HASH_SIZE]
    }

    /// Returns the trigger type for this execution when available.
    pub fn get_trigger_type(&self) -> u8 {
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
    pub fn get_transaction_hash(&self) -> Option<Vec<u8>> {
        None
    }

    /// Returns the current block hash when executed with blockchain context.
    pub fn get_current_block_hash(&self) -> Option<Vec<u8>> {
        None
    }

    /// Returns a storage item when executed with blockchain context.
    pub fn get_storage_item(&self, _key: &[u8]) -> Option<Vec<u8>> {
        None
    }
}
