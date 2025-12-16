//! Block executor types and result structures.

use neo_core::neo_vm::vm_state::VMState;
use neo_core::smart_contract::log_event_args::LogEventArgs;
use neo_core::smart_contract::notify_event_args::NotifyEventArgs;
use neo_core::UInt256;
use neo_state::StateChanges;
use std::fmt;
use thiserror::Error;

/// Errors that can occur during block execution.
#[derive(Debug, Error)]
#[allow(dead_code)] // Variants will be used as executor implementation progresses
pub enum ExecutorError {
    /// Failed to create ApplicationEngine.
    #[error("engine creation failed: {0}")]
    EngineCreation(String),

    /// Script execution failed.
    #[error("execution failed: {0}")]
    ExecutionFailed(String),

    /// Insufficient gas for execution.
    #[error("insufficient gas: required {required}, available {available}")]
    InsufficientGas { required: i64, available: i64 },

    /// State change extraction failed.
    #[error("state extraction failed: {0}")]
    StateExtraction(String),

    /// Contract not found.
    #[error("contract not found: {0}")]
    ContractNotFound(String),

    /// Invalid trigger type.
    #[error("invalid trigger: {0}")]
    InvalidTrigger(String),
}

/// Result type for executor operations.
pub type ExecutorResult<T> = Result<T, ExecutorError>;

/// Result of executing a single transaction.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields will be used when full transaction execution is implemented
pub struct TransactionExecutionResult {
    /// Transaction hash.
    pub tx_hash: UInt256,
    /// Final VM state (HALT or FAULT).
    pub vm_state: VMState,
    /// Total gas consumed (in datoshis).
    pub gas_consumed: i64,
    /// Notifications emitted during execution.
    pub notifications: Vec<NotifyEventArgs>,
    /// Log events emitted during execution.
    pub logs: Vec<LogEventArgs>,
    /// Exception message if execution faulted.
    pub exception: Option<String>,
    /// Storage changes from this transaction.
    pub storage_changes: Vec<(Vec<u8>, Option<Vec<u8>>)>,
}

#[allow(dead_code)] // Methods will be used when full transaction execution is implemented
impl TransactionExecutionResult {
    /// Returns true if execution completed successfully.
    pub fn is_success(&self) -> bool {
        self.vm_state == VMState::HALT
    }

    /// Returns true if execution faulted.
    pub fn is_fault(&self) -> bool {
        self.vm_state == VMState::FAULT
    }
}

/// Result of executing a system trigger (OnPersist/PostPersist).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields will be used when full system execution is implemented
pub struct SystemExecutionResult {
    /// Final VM state.
    pub vm_state: VMState,
    /// Total gas consumed.
    pub gas_consumed: i64,
    /// Notifications emitted.
    pub notifications: Vec<NotifyEventArgs>,
    /// Exception message if faulted.
    pub exception: Option<String>,
    /// Storage changes from system execution.
    pub storage_changes: Vec<(Vec<u8>, Option<Vec<u8>>)>,
}

#[allow(dead_code)] // Methods will be used when full system execution is implemented
impl SystemExecutionResult {
    /// Creates a successful empty result.
    pub fn success() -> Self {
        Self {
            vm_state: VMState::HALT,
            gas_consumed: 0,
            notifications: Vec::new(),
            exception: None,
            storage_changes: Vec::new(),
        }
    }
}

/// Complete result of executing a block.
#[derive(Debug)]
#[allow(dead_code)] // Fields will be used when full block execution is implemented
pub struct BlockExecutionResult {
    /// Block height.
    pub height: u32,
    /// Block hash.
    pub block_hash: UInt256,
    /// OnPersist execution result.
    pub on_persist: SystemExecutionResult,
    /// Results for each transaction.
    pub transactions: Vec<TransactionExecutionResult>,
    /// PostPersist execution result.
    pub post_persist: SystemExecutionResult,
    /// Total gas consumed by all executions.
    pub total_gas_consumed: i64,
    /// Aggregated state changes for state root calculation.
    pub state_changes: StateChanges,
    /// Number of successful transactions.
    pub successful_tx_count: usize,
    /// Number of failed transactions.
    pub failed_tx_count: usize,
}

#[allow(dead_code)] // Methods will be used when full block execution is implemented
impl BlockExecutionResult {
    /// Returns true if all executions completed successfully.
    pub fn is_success(&self) -> bool {
        self.on_persist.vm_state == VMState::HALT
            && self.post_persist.vm_state == VMState::HALT
            && self.transactions.iter().all(|tx| tx.is_success())
    }

    /// Returns the total number of storage changes.
    pub fn total_storage_changes(&self) -> usize {
        self.state_changes.storage.len()
    }
}

impl fmt::Display for BlockExecutionResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "BlockExecution(height={}, txs={}/{}, gas={}, changes={})",
            self.height,
            self.successful_tx_count,
            self.transactions.len(),
            self.total_gas_consumed,
            self.total_storage_changes()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_result_success() {
        let result = TransactionExecutionResult {
            tx_hash: UInt256::zero(),
            vm_state: VMState::HALT,
            gas_consumed: 1000,
            notifications: vec![],
            logs: vec![],
            exception: None,
            storage_changes: vec![],
        };
        assert!(result.is_success());
        assert!(!result.is_fault());
    }

    #[test]
    fn test_transaction_result_fault() {
        let result = TransactionExecutionResult {
            tx_hash: UInt256::zero(),
            vm_state: VMState::FAULT,
            gas_consumed: 500,
            notifications: vec![],
            logs: vec![],
            exception: Some("out of gas".to_string()),
            storage_changes: vec![],
        };
        assert!(!result.is_success());
        assert!(result.is_fault());
    }

    #[test]
    fn test_system_result_success() {
        let result = SystemExecutionResult::success();
        assert_eq!(result.vm_state, VMState::HALT);
        assert_eq!(result.gas_consumed, 0);
    }

    #[test]
    fn test_block_result_display() {
        let result = BlockExecutionResult {
            height: 100,
            block_hash: UInt256::zero(),
            on_persist: SystemExecutionResult::success(),
            transactions: vec![],
            post_persist: SystemExecutionResult::success(),
            total_gas_consumed: 5000,
            state_changes: StateChanges::new(),
            successful_tx_count: 5,
            failed_tx_count: 0,
        };
        let display = format!("{}", result);
        assert!(display.contains("height=100"));
        assert!(display.contains("txs=5/0"));
    }
}
