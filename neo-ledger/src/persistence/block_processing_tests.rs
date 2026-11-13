//! Integration tests for block processing through the persistence pipeline.
//!
//! These tests verify end-to-end block processing scenarios, complementing
//! the operation-level tests in mod.rs.

#[cfg(test)]
mod tests {
    #![allow(dead_code)] // Retain fault-injection controls shared by test scenarios.
    use crate::persistence::*;
    use neo_primitives::{TriggerType, UInt256};
    use neo_protocol::execution::Block;
    use neo_protocol::payloads::Transaction;
    use neo_smart_contract::application_engine::{LedgerTransactionStates, NativeContractsCache};
    use neo_storage::persistence::DataCache;
    use neo_vm::VmState;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    type Log = Arc<Mutex<Vec<String>>>;

    fn log() -> Log {
        Arc::new(Mutex::new(Vec::new()))
    }

    // ---- Test Utilities -------------------------------------------------------

    fn create_block(index: u32, tx_count: usize) -> Block {
        use neo_primitives::{UInt160, UInt256};
        use neo_protocol::execution::BlockHeader;

        let header = BlockHeader::new(
            0,               // version
            UInt256::zero(), // previous_hash
            UInt256::zero(), // merkle_root
            1234567890000,   // timestamp
            0,               // nonce
            index,           // index
            0,               // primary_index
            UInt160::zero(), // next_consensus
            vec![],          // witnesses
        );

        let transactions = (0..tx_count)
            .map(|i| {
                let mut tx = Transaction::new();
                tx.set_nonce((i + 1) as u32);
                tx.set_valid_until_block(100);
                tx.set_script(vec![0x11]);
                tx
            })
            .collect();

        Block::new(header, transactions)
    }

    // ---- Mock Engine Factory --------------------------------------------------

    struct TestNativeEngine {
        trigger: TriggerType,
        fail_on_persist: bool,
        fail_on_post_persist: bool,
        log: Log,
    }

    impl NativeEnginePort for TestNativeEngine {
        fn native_on_persist(&mut self) -> Result<(), LedgerError> {
            self.log.lock().push("on_persist".to_string());
            if self.fail_on_persist {
                return Err(LedgerError::Engine {
                    message: "native persist failed".to_string(),
                });
            }
            Ok(())
        }

        fn native_post_persist(&mut self) -> Result<(), LedgerError> {
            self.log.lock().push("post_persist".to_string());
            if self.fail_on_post_persist {
                return Err(LedgerError::Engine {
                    message: "native persist failed".to_string(),
                });
            }
            Ok(())
        }

        fn record_native_persist_success(&mut self) {}

        fn set_transaction_states(&mut self, _states: LedgerTransactionStates) {}

        fn take_transaction_states(&mut self) -> LedgerTransactionStates {
            LedgerTransactionStates::default()
        }

        fn seed_native_contracts(&self) -> SeededNativeContracts {
            HashMap::new()
        }

        fn native_cache_handle(&self) -> NativeCacheHandle {
            Arc::new(Mutex::new(NativeContractsCache::default()))
        }

        fn execution_receipt(&mut self) -> ExecutionReceipt {
            ExecutionReceipt::native(self.trigger, VmState::HALT, 0)
        }
    }

    struct TestApplicationEngine {
        vm_state: VmState,
        log: Log,
    }

    impl ApplicationEnginePort for TestApplicationEngine {
        fn load_script(&mut self, _script: Vec<u8>) -> Result<(), LedgerError> {
            Ok(())
        }

        fn execute_allow_fault(&mut self) -> VmState {
            self.log.lock().push("execute".to_string());
            self.vm_state
        }

        fn record_transaction_vm_state(&mut self, _tx_hash: &UInt256, _vm_state: VmState) {
            self.log.lock().push("record_state".to_string());
        }

        fn set_transaction_states(&mut self, _states: LedgerTransactionStates) {}

        fn take_transaction_states(&mut self) -> LedgerTransactionStates {
            LedgerTransactionStates::default()
        }

        fn fee_consumed(&self) -> i64 {
            1000
        }

        fn fault_exception(&self) -> Option<String> {
            None
        }

        fn execution_receipt(&mut self) -> ExecutionReceipt {
            ExecutionReceipt {
                trigger: TriggerType::Application,
                transaction_hash: Some(UInt256::zero()),
                vm_state: self.vm_state,
                gas_consumed: 1000,
                exception: None,
            }
        }
    }

    struct TestEngineFactory {
        fail_native_on_persist: bool,
        fail_native_post_persist: bool,
        app_vm_states: Arc<Mutex<Vec<VmState>>>,
        log: Log,
    }

    impl TestEngineFactory {
        fn new(app_vm_states: Vec<VmState>, log: Log) -> Self {
            Self {
                fail_native_on_persist: false,
                fail_native_post_persist: false,
                app_vm_states: Arc::new(Mutex::new(app_vm_states)),
                log,
            }
        }

        fn with_native_on_persist_failure(mut self) -> Self {
            self.fail_native_on_persist = true;
            self
        }

        fn with_native_post_persist_failure(mut self) -> Self {
            self.fail_native_post_persist = true;
            self
        }
    }

    impl EngineFactory for TestEngineFactory {
        fn open_on_persist(
            &self,
            _snapshot: Arc<DataCache>,
            _block: Arc<Block>,
        ) -> Result<Box<dyn NativeEnginePort>, LedgerError> {
            Ok(Box::new(TestNativeEngine {
                trigger: TriggerType::OnPersist,
                fail_on_persist: self.fail_native_on_persist,
                fail_on_post_persist: false,
                log: self.log.clone(),
            }))
        }

        fn open_application(
            &self,
            _container: Arc<dyn neo_primitives::Verifiable>,
            _snapshot: Arc<DataCache>,
            _block: Arc<Block>,
            _system_fee: i64,
            _seeded: &SeededNativeContracts,
            _cache: &NativeCacheHandle,
        ) -> Result<Box<dyn ApplicationEnginePort>, LedgerError> {
            let vm_state = self.app_vm_states.lock().pop().unwrap_or(VmState::HALT);
            Ok(Box::new(TestApplicationEngine {
                vm_state,
                log: self.log.clone(),
            }))
        }

        fn open_post_persist(
            &self,
            _snapshot: Arc<DataCache>,
            _block: Arc<Block>,
            _seeded: &SeededNativeContracts,
            _cache: &NativeCacheHandle,
        ) -> Result<Box<dyn NativeEnginePort>, LedgerError> {
            Ok(Box::new(TestNativeEngine {
                trigger: TriggerType::PostPersist,
                fail_on_persist: self.fail_native_post_persist,
                fail_on_post_persist: self.fail_native_post_persist,
                log: self.log.clone(),
            }))
        }
    }

    // ---- Mock Storage ---------------------------------------------------------

    struct TestTransaction {
        base: Arc<DataCache>,
        commit_should_fail: Arc<AtomicBool>,
        log: Log,
    }

    impl StoreTransactionPort for TestTransaction {
        fn base_snapshot(&self) -> Arc<DataCache> {
            Arc::clone(&self.base)
        }

        fn commit(self: Box<Self>) -> Result<(), String> {
            self.log.lock().push("store_commit".to_string());
            if self.commit_should_fail.load(Ordering::SeqCst) {
                Err("commit failed".to_string())
            } else {
                Ok(())
            }
        }
    }

    struct TestStore {
        commit_should_fail: Arc<AtomicBool>,
        log: Log,
    }

    impl TestStore {
        fn new(log: Log) -> Self {
            Self {
                commit_should_fail: Arc::new(AtomicBool::new(false)),
                log,
            }
        }

        fn set_commit_failure(&self, should_fail: bool) {
            self.commit_should_fail.store(should_fail, Ordering::SeqCst);
        }
    }

    impl SnapshotStore for TestStore {
        fn open_transaction(
            &self,
            _options: &PersistOptions,
        ) -> Result<Box<dyn StoreTransactionPort>, LedgerError> {
            Ok(Box::new(TestTransaction {
                base: Arc::new(DataCache::new(false)),
                commit_should_fail: Arc::clone(&self.commit_should_fail),
                log: self.log.clone(),
            }))
        }
    }

    // ---- Mock Runtime Cache ---------------------------------------------------

    struct TestRuntimeCache {
        record_block_should_fail: Arc<AtomicBool>,
        log: Log,
    }

    impl TestRuntimeCache {
        fn new(log: Log) -> Self {
            Self {
                record_block_should_fail: Arc::new(AtomicBool::new(false)),
                log,
            }
        }

        fn set_record_block_failure(&self, should_fail: bool) {
            self.record_block_should_fail
                .store(should_fail, Ordering::SeqCst);
        }
    }

    impl RuntimeCacheSink for TestRuntimeCache {
        fn record_block(&self, block: &Block) -> Result<(), String> {
            self.log
                .lock()
                .push(format!("record_block:{}", block.index()));
            if self.record_block_should_fail.load(Ordering::SeqCst) {
                Err("cache update failed".to_string())
            } else {
                Ok(())
            }
        }

        fn record_tip(&self, index: u32) {
            self.log.lock().push(format!("record_tip:{}", index));
        }

        fn broadcast_block_received(&self, _hash: String, height: u32) {
            self.log.lock().push(format!("broadcast:{}", height));
        }
    }

    // ---- Mock Evidence Sink ---------------------------------------------------

    struct TestEvidenceSink {
        capacity: usize,
        accept: Arc<AtomicBool>,
        emitted: Arc<AtomicUsize>,
        log: Log,
    }

    impl TestEvidenceSink {
        fn new(capacity: usize, log: Log) -> Self {
            Self {
                capacity,
                accept: Arc::new(AtomicBool::new(true)),
                emitted: Arc::new(AtomicUsize::new(0)),
                log,
            }
        }

        fn set_accept(&self, accept: bool) {
            self.accept.store(accept, Ordering::SeqCst);
        }

        fn emitted_count(&self) -> usize {
            self.emitted.load(Ordering::SeqCst)
        }
    }

    impl EvidenceSink for TestEvidenceSink {
        fn available_capacity(&self) -> usize {
            self.capacity
        }

        fn try_emit(&self, _observation: &FaultObservation) -> bool {
            let accepted = self.accept.load(Ordering::SeqCst);
            if accepted {
                self.emitted.fetch_add(1, Ordering::SeqCst);
                self.log.lock().push("emit_observation".to_string());
            }
            accepted
        }

        fn record_dropped(&self, dropped: u64) {
            self.log.lock().push(format!("dropped:{}", dropped));
        }
    }

    // ---- Test Cases -----------------------------------------------------------

    #[test]
    fn empty_block_processes_only_native_on_and_post_persist() {
        let log = log();
        let store = TestStore::new(log.clone());
        let factory = TestEngineFactory::new(vec![], log.clone());
        let runtime = TestRuntimeCache::new(log.clone());
        let evidence = TestEvidenceSink::new(64, log.clone());

        let receipt = persist_block(
            create_block(0, 0),
            &store,
            &factory,
            &runtime,
            &evidence,
            &[],
            &PersistOptions::online(),
        )
        .expect("empty block should persist");

        assert_eq!(receipt.block_height, 0);
        assert_eq!(receipt.receipts.len(), 2); // OnPersist + PostPersist
        assert_eq!(receipt.staged_fault_observations, 0);

        let events = log.lock();
        assert!(events.contains(&"on_persist".to_string()));
        assert!(events.contains(&"post_persist".to_string()));
        assert!(events.contains(&"store_commit".to_string()));
        assert!(events.contains(&"record_block:0".to_string()));
        assert!(events.contains(&"broadcast:0".to_string()));
    }

    #[test]
    fn block_with_multiple_halt_transactions_generates_correct_receipts() {
        let log = log();
        let store = TestStore::new(log.clone());
        let factory = TestEngineFactory::new(
            vec![VmState::HALT, VmState::HALT, VmState::HALT],
            log.clone(),
        );
        let runtime = TestRuntimeCache::new(log.clone());
        let evidence = TestEvidenceSink::new(64, log.clone());

        let receipt = persist_block(
            create_block(5, 3),
            &store,
            &factory,
            &runtime,
            &evidence,
            &[],
            &PersistOptions::online(),
        )
        .expect("block with halting transactions should persist");

        assert_eq!(receipt.block_height, 5);
        assert_eq!(receipt.receipts.len(), 5); // OnPersist + 3 tx + PostPersist
        assert_eq!(receipt.staged_fault_observations, 0);
        assert_eq!(evidence.emitted_count(), 0);
    }

    #[test]
    fn mixed_halt_and_fault_transactions_emit_fault_observations() {
        let log = log();
        let store = TestStore::new(log.clone());
        let factory = TestEngineFactory::new(
            vec![
                VmState::HALT,
                VmState::FAULT,
                VmState::HALT,
                VmState::FAULT,
                VmState::FAULT,
            ],
            log.clone(),
        );
        let runtime = TestRuntimeCache::new(log.clone());
        let evidence = TestEvidenceSink::new(64, log.clone());

        let receipt = persist_block(
            create_block(10, 5),
            &store,
            &factory,
            &runtime,
            &evidence,
            &[],
            &PersistOptions::online(),
        )
        .expect("mixed block should persist");

        assert_eq!(receipt.staged_fault_observations, 3); // 3 faults
        assert_eq!(evidence.emitted_count(), 3);
        assert!(log.lock().contains(&"store_commit".to_string()));
    }

    #[test]
    fn native_on_persist_failure_aborts_before_transaction_loop() {
        let log = log();
        let store = TestStore::new(log.clone());
        let factory = TestEngineFactory::new(vec![VmState::HALT], log.clone())
            .with_native_on_persist_failure();
        let runtime = TestRuntimeCache::new(log.clone());
        let evidence = TestEvidenceSink::new(64, log.clone());

        let err = persist_block(
            create_block(0, 1),
            &store,
            &factory,
            &runtime,
            &evidence,
            &[],
            &PersistOptions::online(),
        )
        .expect_err("native on_persist failure should abort");

        assert!(err.to_string().contains("native persist failed"));
        let events = log.lock();
        // Should not reach transaction execution or store commit
        assert!(!events.iter().any(|e| e == "execute"));
        assert!(!events.contains(&"store_commit".to_string()));
    }

    #[test]
    fn native_post_persist_failure_aborts_after_transactions() {
        let log = log();
        let store = TestStore::new(log.clone());
        let factory = TestEngineFactory::new(vec![VmState::HALT], log.clone())
            .with_native_post_persist_failure();
        let runtime = TestRuntimeCache::new(log.clone());
        let evidence = TestEvidenceSink::new(64, log.clone());

        let err = persist_block(
            create_block(0, 1),
            &store,
            &factory,
            &runtime,
            &evidence,
            &[],
            &PersistOptions::online(),
        )
        .expect_err("native post_persist failure should abort");

        assert!(err.to_string().contains("native persist failed"));
        let events = log.lock();
        // Transaction should have executed
        assert!(events.contains(&"execute".to_string()));
        // But should not reach store commit
        assert!(!events.contains(&"store_commit".to_string()));
    }

    #[test]
    fn evidence_capacity_overflow_drops_observations_gracefully() {
        let log = log();
        let store = TestStore::new(log.clone());
        let factory = TestEngineFactory::new(vec![VmState::FAULT; 10], log.clone());
        let runtime = TestRuntimeCache::new(log.clone());
        let evidence = TestEvidenceSink::new(3, log.clone()); // Only 3 slots

        let receipt = persist_block(
            create_block(0, 10),
            &store,
            &factory,
            &runtime,
            &evidence,
            &[],
            &PersistOptions::online(),
        )
        .expect("overflow should not fail persistence");

        assert_eq!(receipt.staged_fault_observations, 3); // Capped at capacity
        assert_eq!(receipt.dropped_fault_observations, 7); // 7 dropped
        assert_eq!(evidence.emitted_count(), 3);
    }

    #[test]
    fn offline_import_mode_skips_runtime_block_cache_update() {
        let log = log();
        let store = TestStore::new(log.clone());
        let factory = TestEngineFactory::new(vec![VmState::HALT], log.clone());
        let runtime = TestRuntimeCache::new(log.clone());
        let evidence = TestEvidenceSink::new(64, log.clone());

        let receipt = persist_block(
            create_block(7, 1),
            &store,
            &factory,
            &runtime,
            &evidence,
            &[],
            &PersistOptions::offline_import(),
        )
        .expect("offline import should persist");

        let events = log.lock();
        assert!(events.contains(&"record_tip:7".to_string()));
        assert!(!events.iter().any(|e| e.starts_with("record_block:")));
        // Still emits detailed receipts and broadcast in offline mode
        assert!(receipt.detailed);
        assert!(events.contains(&"broadcast:7".to_string()));
    }

    #[test]
    fn fast_sync_mode_suppresses_broadcast_when_no_participant_opts_in() {
        let log = log();
        let store = TestStore::new(log.clone());
        let factory = TestEngineFactory::new(vec![VmState::HALT], log.clone());
        let runtime = TestRuntimeCache::new(log.clone());
        let evidence = TestEvidenceSink::new(64, log.clone());

        let receipt = persist_block(
            create_block(0, 1),
            &store,
            &factory,
            &runtime,
            &evidence,
            &[],
            &PersistOptions::fast_sync(),
        )
        .expect("fast-sync should persist");

        assert!(!receipt.detailed);
        assert!(receipt.receipts.is_empty());
        let events = log.lock();
        assert!(!events.iter().any(|e| e.starts_with("broadcast:")));
        // But still records VM state
        assert!(events.contains(&"record_state".to_string()));
    }

    #[test]
    fn transaction_hash_computation_failure_aborts_early() {
        let log = log();
        let store = TestStore::new(log.clone());
        let factory = TestEngineFactory::new(vec![], log.clone());
        let runtime = TestRuntimeCache::new(log.clone());
        let evidence = TestEvidenceSink::new(64, log.clone());

        // Create a block with an invalid transaction that cannot be hashed
        let mut block = Block::default();
        let mut tx = Transaction::new();
        tx.set_script(vec![]); // Empty script might cause issues
        tx.set_valid_until_block(0); // Invalid
        block.transactions = vec![tx];

        // The test might succeed if hash computation doesn't fail
        // This test documents the expected behavior if it does fail
        let result = persist_block(
            block,
            &store,
            &factory,
            &runtime,
            &evidence,
            &[],
            &PersistOptions::online(),
        );

        // Either succeeds with empty script or fails early
        match result {
            Ok(_) => {
                // Hash computation succeeded, verify normal flow
                assert!(log.lock().contains(&"store_commit".to_string()));
            }
            Err(err) => {
                // Hash computation failed, verify early abort
                assert!(matches!(err, LedgerError::TransactionHash { .. }));
                assert!(!log.lock().contains(&"store_commit".to_string()));
            }
        }
    }

    #[test]
    fn block_processing_order_is_deterministic() {
        let log = log();
        let store = TestStore::new(log.clone());
        let factory = TestEngineFactory::new(
            vec![VmState::HALT, VmState::FAULT, VmState::HALT],
            log.clone(),
        );
        let runtime = TestRuntimeCache::new(log.clone());
        let evidence = TestEvidenceSink::new(64, log.clone());

        let _receipt = persist_block(
            create_block(0, 3),
            &store,
            &factory,
            &runtime,
            &evidence,
            &[],
            &PersistOptions::online(),
        )
        .expect("should persist");

        let events = log.lock();
        let on_persist_pos = events
            .iter()
            .position(|e| e == "on_persist")
            .expect("on_persist");
        let first_execute = events.iter().position(|e| e == "execute").expect("execute");
        let post_persist_pos = events
            .iter()
            .position(|e| e == "post_persist")
            .expect("post_persist");
        let commit_pos = events
            .iter()
            .position(|e| e == "store_commit")
            .expect("commit");
        let broadcast_pos = events
            .iter()
            .position(|e| e.starts_with("broadcast:"))
            .expect("broadcast");

        // Verify canonical order
        assert!(on_persist_pos < first_execute);
        assert!(first_execute < post_persist_pos);
        assert!(post_persist_pos < commit_pos);
        assert!(commit_pos < broadcast_pos);
    }
}
