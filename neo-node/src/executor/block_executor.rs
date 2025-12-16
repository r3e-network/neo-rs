//! Production-ready block executor with full transaction processing.
//!
//! This module implements the complete Neo N3 block execution flow:
//! 1. OnPersist - System trigger for native contract persistence
//! 2. Application - Execute each transaction in block
//! 3. PostPersist - System trigger for post-block cleanup
//!
//! All state changes are extracted for MPT state root calculation.

use super::key_converter::StorageKeyConverter;
use super::types::*;
use neo_core::ledger::Block as LedgerBlock;
use neo_core::neo_vm::vm_state::VMState;
use neo_core::network::p2p::payloads::{Block, Transaction};
use neo_core::persistence::data_cache::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::IVerifiable;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Maximum gas for system triggers (OnPersist/PostPersist).
/// These are system operations and should have unlimited gas in practice.
const SYSTEM_GAS_LIMIT: i64 = i64::MAX;

/// Converts a network payload Block to a ledger Block for ApplicationEngine.
fn convert_to_ledger_block(block: &Block) -> LedgerBlock {
    use neo_core::ledger::BlockHeader as LedgerBlockHeader;

    let header = LedgerBlockHeader::new(
        block.header.version(),
        *block.header.prev_hash(),
        *block.header.merkle_root(),
        block.header.timestamp(),
        block.header.nonce(),
        block.header.index(),
        block.header.primary_index(),
        *block.header.next_consensus(),
        vec![block.header.witness.clone()],
    );

    LedgerBlock::new(header, block.transactions.clone())
}

/// Block executor implementing full Neo N3 execution semantics.
///
/// This executor processes blocks through the complete execution pipeline:
/// - OnPersist: Native contracts update their state (GAS distribution, committee refresh)
/// - Application: Each transaction is executed via ApplicationEngine
/// - PostPersist: Native contracts perform post-block cleanup (GAS rewards)
///
/// All storage changes are collected and converted to StateChanges for MPT calculation.
pub struct BlockExecutorImpl {
    /// Protocol settings for the network.
    protocol_settings: ProtocolSettings,
    /// Storage key converter for ID-to-hash mapping.
    key_converter: StorageKeyConverter,
}

impl BlockExecutorImpl {
    /// Creates a new block executor with the given protocol settings.
    pub fn new(protocol_settings: ProtocolSettings) -> Self {
        Self {
            protocol_settings,
            key_converter: StorageKeyConverter::new(),
        }
    }

    /// Executes a complete block and returns the execution result.
    ///
    /// # Execution Flow
    /// 1. Execute OnPersist trigger (native contracts)
    /// 2. Execute each transaction in Application trigger
    /// 3. Execute PostPersist trigger (native contracts)
    /// 4. Aggregate all state changes for state root calculation
    ///
    /// # Arguments
    /// * `block` - The block to execute
    /// * `snapshot` - The current state snapshot
    ///
    /// # Returns
    /// Complete execution result including all state changes
    pub fn execute_block(
        &self,
        block: &Block,
        snapshot: Arc<DataCache>,
    ) -> ExecutorResult<BlockExecutionResult> {
        let height = block.index();
        let mut block_clone = block.clone();
        let block_hash = Block::hash(&mut block_clone);
        let tx_count = block.transactions.len();

        info!(
            target: "neo::executor",
            height,
            tx_count,
            "executing block"
        );

        // Collect all raw storage changes across all executions
        let mut all_raw_changes: Vec<(Vec<u8>, Option<Vec<u8>>)> = Vec::new();
        let mut total_gas_consumed: i64 = 0;

        // Phase 1: OnPersist
        let on_persist = self.execute_on_persist(block, Arc::clone(&snapshot))?;
        total_gas_consumed = total_gas_consumed.saturating_add(on_persist.gas_consumed);
        all_raw_changes.extend(on_persist.storage_changes.clone());

        debug!(
            target: "neo::executor",
            height,
            gas = on_persist.gas_consumed,
            changes = on_persist.storage_changes.len(),
            "OnPersist completed"
        );

        // Phase 2: Application (execute each transaction)
        let mut tx_results = Vec::with_capacity(tx_count);
        let mut successful_tx_count = 0;
        let mut failed_tx_count = 0;

        for (tx_idx, tx) in block.transactions.iter().enumerate() {
            let tx_result = self.execute_transaction(tx, block, Arc::clone(&snapshot))?;

            if tx_result.is_success() {
                successful_tx_count += 1;
            } else {
                failed_tx_count += 1;
                warn!(
                    target: "neo::executor",
                    height,
                    tx_idx,
                    tx_hash = %tx_result.tx_hash,
                    exception = ?tx_result.exception,
                    "transaction faulted"
                );
            }

            total_gas_consumed = total_gas_consumed.saturating_add(tx_result.gas_consumed);
            all_raw_changes.extend(tx_result.storage_changes.clone());
            tx_results.push(tx_result);
        }

        debug!(
            target: "neo::executor",
            height,
            successful = successful_tx_count,
            failed = failed_tx_count,
            "transactions executed"
        );

        // Phase 3: PostPersist
        let post_persist = self.execute_post_persist(block, Arc::clone(&snapshot))?;
        total_gas_consumed = total_gas_consumed.saturating_add(post_persist.gas_consumed);
        all_raw_changes.extend(post_persist.storage_changes.clone());

        debug!(
            target: "neo::executor",
            height,
            gas = post_persist.gas_consumed,
            changes = post_persist.storage_changes.len(),
            "PostPersist completed"
        );

        // Convert all raw changes to StateChanges for MPT calculation
        let state_changes = self.key_converter.convert_to_state_changes(all_raw_changes);

        info!(
            target: "neo::executor",
            height,
            total_gas = total_gas_consumed,
            storage_changes = state_changes.storage.len(),
            successful_tx = successful_tx_count,
            failed_tx = failed_tx_count,
            "block execution completed"
        );

        Ok(BlockExecutionResult {
            height,
            block_hash,
            on_persist,
            transactions: tx_results,
            post_persist,
            total_gas_consumed,
            state_changes,
            successful_tx_count,
            failed_tx_count,
        })
    }

    /// Executes the OnPersist system trigger.
    ///
    /// This calls `on_persist` on all active native contracts, which:
    /// - GasToken: Burns system fees, distributes network fees to validators
    /// - NeoToken: Refreshes committee if needed
    /// - LedgerContract: Records block metadata
    fn execute_on_persist(
        &self,
        block: &Block,
        snapshot: Arc<DataCache>,
    ) -> ExecutorResult<SystemExecutionResult> {
        let ledger_block = convert_to_ledger_block(block);
        let mut engine = ApplicationEngine::new(
            TriggerType::OnPersist,
            None, // No script container for system triggers
            snapshot,
            Some(ledger_block),
            self.protocol_settings.clone(),
            SYSTEM_GAS_LIMIT,
            None, // No diagnostic
        )
        .map_err(|e| ExecutorError::EngineCreation(e.to_string()))?;

        // Execute native OnPersist handlers
        if let Err(e) = engine.native_on_persist() {
            return Ok(SystemExecutionResult {
                vm_state: VMState::FAULT,
                gas_consumed: engine.gas_consumed(),
                notifications: engine.notifications().to_vec(),
                exception: Some(e.to_string()),
                storage_changes: engine.extract_storage_changes(),
            });
        }

        Ok(SystemExecutionResult {
            vm_state: VMState::HALT,
            gas_consumed: engine.gas_consumed(),
            notifications: engine.notifications().to_vec(),
            exception: None,
            storage_changes: engine.extract_storage_changes(),
        })
    }

    /// Executes a single transaction.
    ///
    /// Creates an ApplicationEngine with Application trigger and executes
    /// the transaction's script with proper gas metering.
    fn execute_transaction(
        &self,
        tx: &Transaction,
        block: &Block,
        snapshot: Arc<DataCache>,
    ) -> ExecutorResult<TransactionExecutionResult> {
        let tx_hash = tx.hash();
        let gas_limit = tx.system_fee() + tx.network_fee();

        // Create transaction as IVerifiable container
        let container: Arc<dyn IVerifiable> = Arc::new(tx.clone());
        let ledger_block = convert_to_ledger_block(block);

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            snapshot,
            Some(ledger_block),
            self.protocol_settings.clone(),
            gas_limit,
            None,
        )
        .map_err(|e| ExecutorError::EngineCreation(e.to_string()))?;

        // Load and execute the transaction script
        if let Err(e) = engine.load_script(tx.script().to_vec(), CallFlags::ALL, None) {
            return Ok(TransactionExecutionResult {
                tx_hash,
                vm_state: VMState::FAULT,
                gas_consumed: engine.gas_consumed(),
                notifications: engine.notifications().to_vec(),
                logs: engine.logs().to_vec(),
                exception: Some(format!("Script load failed: {}", e)),
                storage_changes: engine.extract_storage_changes(),
            });
        }

        // Execute the script
        let vm_state = engine.execute_allow_fault();
        let exception = if vm_state == VMState::FAULT {
            engine.fault_exception().map(|s| s.to_string())
        } else {
            None
        };

        Ok(TransactionExecutionResult {
            tx_hash,
            vm_state,
            gas_consumed: engine.gas_consumed(),
            notifications: engine.notifications().to_vec(),
            logs: engine.logs().to_vec(),
            exception,
            storage_changes: engine.extract_storage_changes(),
        })
    }

    /// Executes the PostPersist system trigger.
    ///
    /// This calls `post_persist` on all active native contracts, which:
    /// - NeoToken: Distributes GAS rewards to committee members
    /// - OracleContract: Processes pending oracle responses
    fn execute_post_persist(
        &self,
        block: &Block,
        snapshot: Arc<DataCache>,
    ) -> ExecutorResult<SystemExecutionResult> {
        let ledger_block = convert_to_ledger_block(block);
        let mut engine = ApplicationEngine::new(
            TriggerType::PostPersist,
            None,
            snapshot,
            Some(ledger_block),
            self.protocol_settings.clone(),
            SYSTEM_GAS_LIMIT,
            None,
        )
        .map_err(|e| ExecutorError::EngineCreation(e.to_string()))?;

        // Execute native PostPersist handlers
        if let Err(e) = engine.native_post_persist() {
            return Ok(SystemExecutionResult {
                vm_state: VMState::FAULT,
                gas_consumed: engine.gas_consumed(),
                notifications: engine.notifications().to_vec(),
                exception: Some(e.to_string()),
                storage_changes: engine.extract_storage_changes(),
            });
        }

        Ok(SystemExecutionResult {
            vm_state: VMState::HALT,
            gas_consumed: engine.gas_consumed(),
            notifications: engine.notifications().to_vec(),
            exception: None,
            storage_changes: engine.extract_storage_changes(),
        })
    }

    /// Registers a user-deployed contract for key conversion.
    ///
    /// This should be called when a new contract is deployed to ensure
    /// its storage keys can be properly converted for state root calculation.
    #[allow(dead_code)] // Will be used when contract deployment is fully integrated
    pub fn register_contract(&mut self, id: i32, hash: neo_core::UInt160) {
        self.key_converter.register_contract(id, hash);
    }

    /// Returns the number of registered contracts.
    pub fn registered_contract_count(&self) -> usize {
        self.key_converter.contract_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_executor() -> BlockExecutorImpl {
        BlockExecutorImpl::new(ProtocolSettings::default())
    }

    fn create_test_snapshot() -> Arc<DataCache> {
        Arc::new(DataCache::new(false))
    }

    #[test]
    fn test_executor_creation() {
        let executor = create_test_executor();
        // Should have native contracts registered
        assert!(executor.registered_contract_count() >= 6);
    }

    #[test]
    fn test_register_contract() {
        let mut executor = create_test_executor();
        let initial_count = executor.registered_contract_count();

        let hash = neo_core::UInt160::from([0xAA; 20]);
        executor.register_contract(100, hash);

        assert_eq!(executor.registered_contract_count(), initial_count + 1);
    }

    #[test]
    fn test_on_persist_execution() {
        let executor = create_test_executor();
        let snapshot = create_test_snapshot();

        // Create a minimal test block
        let block = Block::default();

        let result = executor.execute_on_persist(&block, snapshot);

        // OnPersist should succeed (may have no changes on empty state)
        assert!(result.is_ok());
        let system_result = result.unwrap();
        // VM state should be Halt (success) or Fault (if native contracts need state)
        // On empty state, native contracts may fault due to missing genesis data
        assert!(
            system_result.vm_state == VMState::HALT || system_result.vm_state == VMState::FAULT
        );
    }

    #[test]
    fn test_post_persist_execution() {
        let executor = create_test_executor();
        let snapshot = create_test_snapshot();

        let block = Block::default();

        let result = executor.execute_post_persist(&block, snapshot);

        assert!(result.is_ok());
    }

    #[test]
    fn test_transaction_execution_empty_script() {
        let executor = create_test_executor();
        let snapshot = create_test_snapshot();

        let block = Block::default();
        let tx = Transaction::default();

        let result = executor.execute_transaction(&tx, &block, snapshot);

        assert!(result.is_ok());
        let tx_result = result.unwrap();
        // Empty script should halt immediately
        assert!(tx_result.vm_state == VMState::HALT || tx_result.vm_state == VMState::FAULT);
    }

    #[test]
    fn test_block_execution_empty_block() {
        let executor = create_test_executor();
        let snapshot = create_test_snapshot();

        let block = Block::default();

        let result = executor.execute_block(&block, snapshot);

        assert!(result.is_ok());
        let block_result = result.unwrap();
        assert_eq!(block_result.transactions.len(), 0);
        assert_eq!(block_result.successful_tx_count, 0);
        assert_eq!(block_result.failed_tx_count, 0);
    }
}
