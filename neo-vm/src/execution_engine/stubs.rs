//
// stubs.rs - Stub methods for ApplicationEngine override
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

    /// Gets current script hash (stub implementation for base ExecutionEngine)
    /// ApplicationEngine overrides this with actual script hash tracking
    pub fn current_script_hash(&self) -> Option<&[u8]> {
        // Base implementation returns None
        None
    }

    /// Gets script container (stub implementation for base ExecutionEngine)
    /// ApplicationEngine overrides this with actual script container
    pub fn get_script_container(&self) -> Option<&dyn std::any::Any> {
        // Base implementation returns None
        None
    }

    /// Gets the script container hash for signature verification.
    /// Returns the hash of the current transaction or block being executed.
    pub fn get_script_container_hash(&self) -> Vec<u8> {
        // Base implementation returns empty hash
        // ApplicationEngine overrides this with actual container hash
        vec![0u8; HASH_SIZE]
    }

    /// Gets the trigger type for this execution (stub implementation for base ExecutionEngine)
    /// ApplicationEngine overrides this with actual trigger type
    pub fn get_trigger_type(&self) -> u8 {
        0x40
    }

    /// Emits a runtime log event (stub implementation for base ExecutionEngine)
    /// ApplicationEngine overrides this with actual event emission
    pub fn emit_runtime_log_event(&mut self, _message: &str) -> VmResult<()> {
        // Base implementation does nothing
        Ok(())
    }

    /// Adds an execution log (stub implementation for base ExecutionEngine)
    /// ApplicationEngine overrides this with actual log tracking
    pub fn add_execution_log(&mut self, _message: String) -> VmResult<()> {
        // Base implementation does nothing
        Ok(())
    }

    /// Gets transaction hash (stub implementation for base ExecutionEngine)
    /// ApplicationEngine overrides this with actual transaction access
    pub fn get_transaction_hash(&self) -> Option<Vec<u8>> {
        // Base implementation returns None
        None
    }

    /// Gets current block hash (stub implementation for base ExecutionEngine)
    /// ApplicationEngine overrides this with actual blockchain access
    pub fn get_current_block_hash(&self) -> Option<Vec<u8>> {
        // Base implementation returns None
        None
    }

    /// Gets storage item (stub implementation for base ExecutionEngine)
    /// ApplicationEngine overrides this with actual storage access
    pub fn get_storage_item(&self, _key: &[u8]) -> Option<Vec<u8>> {
        // Base implementation returns None
        None
    }
}
