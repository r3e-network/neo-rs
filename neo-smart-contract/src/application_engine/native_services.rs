//! Native capabilities consumed by an engine without concrete native dependencies.

use super::{ApplicationEngine, NativeContract};
use crate::contract_state::ContractState;
use crate::error::CoreResult;
use indexmap::IndexMap;
use neo_primitives::LogEventArgs;
use neo_primitives::{UInt160, UInt256};
use neo_protocol::signer::Signer;
use neo_storage::persistence::DataCache;
use neo_vm::NotifyEventArgs;
use std::sync::Arc;

/// Composition-specific chain reads, attached to an individual execution engine.
pub trait NativeChainQueries: Send + Sync {
    /// Returns the composition's current chain height.
    fn current_block_index(&self) -> u32;
    /// Returns the hash at the supplied height, if available.
    fn block_hash_at(&self, index: u32) -> Option<UInt256>;
}

/// Snapshot-backed native state reads supplied by the composition layer.
pub trait NativeStateReader: Send + Sync {
    /// Reads the actual deployed contract record from the supplied snapshot.
    fn contract(&self, snapshot: &DataCache, hash: &UInt160) -> CoreResult<Option<ContractState>>;
    /// Reads the persisted ledger height.
    fn current_index(&self, snapshot: &DataCache) -> CoreResult<u32>;
    /// Checks the actual policy blocked-account state.
    fn is_blocked(&self, snapshot: &DataCache, hash: &UInt160) -> CoreResult<bool>;
    /// Reads the Faun whitelist fee for a specific method signature.
    fn whitelisted_fee(
        &self,
        snapshot: &DataCache,
        hash: &UInt160,
        method: &str,
        param_count: u32,
    ) -> CoreResult<Option<i64>>;
    /// Resolves the original transaction signers of an oracle response.
    fn oracle_original_signers(
        &self,
        snapshot: &DataCache,
        id: u64,
    ) -> CoreResult<Option<Vec<Signer>>>;
}

/// Ledger-owned verifying-hash resolution, separate from native state reads.
pub trait WitnessHashResolver: Send + Sync {
    /// Resolves verifying hashes using the actual supplied snapshot.
    fn resolve(
        &self,
        verifiable: &dyn neo_primitives::Verifiable,
        snapshot: &DataCache,
    ) -> Vec<UInt160>;
}

/// Event-only observer; it cannot expose a running system or ledger services.
pub trait EngineEventObserver: Send + Sync {
    /// Observes a log before the engine retains it.
    fn notify_application_log(&self, engine: &ApplicationEngine, event: &LogEventArgs);
    /// Observes a notification before the engine retains it.
    fn notify_application_notify(&self, engine: &ApplicationEngine, event: &NotifyEventArgs);
}

/// Native services moved into each engine at construction.
#[derive(Clone)]
pub struct EngineNativeServices {
    /// The ordered registry owned by this engine.
    pub registry: NativeRegistry,
    /// Snapshot-backed reads; no snapshots are captured by the engine interface.
    pub state_reader: Arc<dyn NativeStateReader>,
    /// Verifying hashes are resolved by a higher-layer adapter.
    pub witness_hash_resolver: Arc<dyn WitnessHashResolver>,
    /// The configured policy contract's hash.
    pub policy_hash: UInt160,
    /// Legacy execution units; the engine retains its existing picoGAS scaling.
    pub default_exec_fee_factor: u32,
    /// Initial storage price in datoshi per byte.
    pub default_storage_price: u32,
}

/// Ordered native interface container, without standard contract construction.
#[derive(Clone)]
pub struct NativeRegistry {
    contracts: IndexMap<UInt160, Arc<dyn NativeContract>>,
}

impl NativeRegistry {
    /// Creates an empty registry for explicit composition.
    pub fn empty() -> Self {
        Self {
            contracts: IndexMap::new(),
        }
    }

    /// Registers a native contract.
    pub fn register(&mut self, contract: Arc<dyn NativeContract>) {
        let hash = contract.hash();
        self.contracts.insert(hash, contract);
    }

    /// Gets a native contract by hash.
    pub fn get(&self, hash: &UInt160) -> Option<Arc<dyn NativeContract>> {
        self.contracts.get(hash).cloned()
    }

    /// Gets a native contract by name.
    pub fn get_by_name(&self, name: &str) -> Option<Arc<dyn NativeContract>> {
        self.contracts
            .values()
            .find(|contract| contract.name().eq_ignore_ascii_case(name))
            .cloned()
    }

    fn find_hash_by_name(&self, name: &str) -> Option<UInt160> {
        self.contracts
            .iter()
            .find(|(_, contract)| contract.name().eq_ignore_ascii_case(name))
            .map(|(hash, _)| *hash)
    }

    /// Removes and returns the registered native contract whose name matches (case-insensitive).
    pub fn take_contract_by_name(&mut self, name: &str) -> Option<Arc<dyn NativeContract>> {
        let hash = self.find_hash_by_name(name)?;
        self.contracts.shift_remove(&hash)
    }

    /// Checks if a contract hash is a native contract.
    pub fn is_native(&self, hash: &UInt160) -> bool {
        self.contracts.contains_key(hash)
    }

    /// Gets all native contract hashes.
    pub fn all_hashes(&self) -> Vec<UInt160> {
        self.contracts.keys().copied().collect()
    }

    /// Returns all registered native contracts in deterministic registration order.
    ///
    /// Persistence order is consensus-critical. This follows the same declaration order
    /// as neo-project/neo `NativeContract.Contracts`.
    pub fn contracts(&self) -> impl Iterator<Item = Arc<dyn NativeContract>> + '_ {
        self.contracts.values().cloned()
    }
}

impl Default for NativeRegistry {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application_engine::{
        FEE_FACTOR, LedgerTransactionStates, NativeContractsCache, NativeMethod, TEST_MODE_GAS,
    };
    use crate::contract_state::NefFile;
    use crate::manifest::ContractManifest;
    use neo_primitives::TriggerType;
    use neo_primitives::{UInt256, Verifiable};
    use neo_protocol::ProtocolSettings;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct SnapshotReader {
        snapshot: Arc<DataCache>,
        height_reads: AtomicUsize,
    }

    impl NativeStateReader for SnapshotReader {
        fn contract(
            &self,
            _snapshot: &DataCache,
            _hash: &UInt160,
        ) -> CoreResult<Option<ContractState>> {
            panic!("unexpected contract read during injected construction")
        }

        fn current_index(&self, snapshot: &DataCache) -> CoreResult<u32> {
            assert!(std::ptr::eq(snapshot, self.snapshot.as_ref()));
            self.height_reads.fetch_add(1, Ordering::SeqCst);
            Ok(0)
        }

        fn is_blocked(&self, _snapshot: &DataCache, _hash: &UInt160) -> CoreResult<bool> {
            panic!("unexpected policy read during injected construction")
        }

        fn whitelisted_fee(
            &self,
            _snapshot: &DataCache,
            _hash: &UInt160,
            _method: &str,
            _param_count: u32,
        ) -> CoreResult<Option<i64>> {
            panic!("unexpected whitelist read during injected construction")
        }

        fn oracle_original_signers(
            &self,
            _snapshot: &DataCache,
            _id: u64,
        ) -> CoreResult<Option<Vec<Signer>>> {
            panic!("unexpected oracle read during injected construction")
        }
    }

    struct HashContainer;

    impl Verifiable for HashContainer {
        fn verify(&self) -> bool {
            true
        }

        fn hash(&self) -> neo_primitives::PrimitiveResult<UInt256> {
            Ok(UInt256::zero())
        }

        fn hash_data(&self) -> Vec<u8> {
            Vec::new()
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    struct SnapshotResolver {
        snapshot: Mutex<Arc<DataCache>>,
        hash: UInt160,
        calls: AtomicUsize,
    }

    impl WitnessHashResolver for SnapshotResolver {
        fn resolve(&self, verifiable: &dyn Verifiable, snapshot: &DataCache) -> Vec<UInt160> {
            assert!(std::ptr::eq(snapshot, self.snapshot.lock().as_ref()));
            assert!(verifiable.as_any().is::<HashContainer>());
            self.calls.fetch_add(1, Ordering::SeqCst);
            vec![self.hash]
        }
    }

    struct MetadataNative {
        hash: UInt160,
        metadata_reads: AtomicUsize,
    }

    impl NativeContract for MetadataNative {
        fn id(&self) -> i32 {
            -100
        }

        fn hash(&self) -> UInt160 {
            self.hash
        }

        fn name(&self) -> &str {
            "InjectedNative"
        }

        fn methods(&self) -> &[NativeMethod] {
            &[]
        }

        fn contract_state(
            &self,
            _settings: &ProtocolSettings,
            _height: u32,
        ) -> Option<ContractState> {
            self.metadata_reads.fetch_add(1, Ordering::SeqCst);
            Some(ContractState::new(
                self.id(),
                self.hash,
                NefFile::new(
                    "injected-test".to_string(),
                    vec![neo_vm::OpCode::RET.byte()],
                ),
                ContractManifest::new_native(self.name().to_string()),
            ))
        }

        fn invoke(
            &self,
            _engine: &mut ApplicationEngine,
            _method: &str,
            _args: &[Vec<u8>],
        ) -> CoreResult<Vec<u8>> {
            panic!("unexpected native invocation during injected construction")
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    fn services(
        registry: NativeRegistry,
        reader: Arc<SnapshotReader>,
        resolver: Arc<SnapshotResolver>,
    ) -> EngineNativeServices {
        EngineNativeServices {
            registry,
            state_reader: reader,
            witness_hash_resolver: resolver,
            policy_hash: UInt160::zero(),
            default_exec_fee_factor: 31,
            default_storage_price: 100_001,
        }
    }

    fn ports(snapshot: &Arc<DataCache>) -> (Arc<SnapshotReader>, Arc<SnapshotResolver>) {
        (
            Arc::new(SnapshotReader {
                snapshot: Arc::clone(snapshot),
                height_reads: AtomicUsize::new(0),
            }),
            Arc::new(SnapshotResolver {
                snapshot: Mutex::new(Arc::clone(snapshot)),
                hash: UInt160::from_bytes(&[0x42; 20]).expect("hash"),
                calls: AtomicUsize::new(0),
            }),
        )
    }

    #[test]
    fn registry_empty_and_default_do_not_construct_standard_contracts() {
        for mut registry in [NativeRegistry::empty(), NativeRegistry::default()] {
            assert!(registry.all_hashes().is_empty());
            assert_eq!(registry.contracts().count(), 0);
            assert!(registry.get(&UInt160::zero()).is_none());
            assert!(registry.get_by_name("ContractManagement").is_none());
            assert!(registry.take_contract_by_name("NeoToken").is_none());
        }
    }

    #[test]
    fn shared_constructor_consumes_registry_and_separate_snapshot_ports() {
        let snapshot = Arc::new(DataCache::new(false));
        let (reader, resolver) = ports(&snapshot);
        let native = Arc::new(MetadataNative {
            hash: UInt160::from_bytes(&[0x41; 20]).expect("hash"),
            metadata_reads: AtomicUsize::new(0),
        });
        let mut registry = NativeRegistry::empty();
        registry.register(native.clone());
        let mut engine = ApplicationEngine::new_with_shared_block(
            TriggerType::Application,
            Some(Arc::new(HashContainer)),
            Arc::clone(&snapshot),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            None,
            services(registry, reader.clone(), resolver.clone()),
        )
        .expect("injected engine");
        assert!(engine.is_native_contract_hash(&native.hash));
        assert_eq!(engine.native_registry.contracts().count(), 1);
        assert_eq!(native.metadata_reads.load(Ordering::SeqCst), 1);
        assert!(reader.height_reads.load(Ordering::SeqCst) > 0);
        assert_eq!(engine.exec_fee_factor_raw(), 31 * FEE_FACTOR as u32);
        assert_eq!(engine.storage_price(), 100_001);
        assert_eq!(engine.fee_consumed(), 0);
        assert_eq!(engine.gas_consumed(), 0);
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            engine
                .check_witness_hash(&resolver.hash)
                .unwrap_err()
                .to_string(),
            "Invalid operation: Read states not allowed"
        );
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
        engine
            .load_script(
                vec![neo_vm::OpCode::RET.byte()],
                neo_primitives::CallFlags::READ_STATES,
                None,
            )
            .expect("load witness context");
        let context_snapshot = engine.snapshot_cache();
        assert!(!Arc::ptr_eq(&context_snapshot, &snapshot));
        *resolver.snapshot.lock() = context_snapshot;
        assert!(
            engine
                .check_witness_hash(&resolver.hash)
                .expect("injected witness")
        );
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
        engine.set_state(LedgerTransactionStates::default());
        assert!(engine.get_state::<LedgerTransactionStates>().is_some());
        assert!(
            engine
                .take_state::<LedgerTransactionStates>()
                .unwrap()
                .states()
                .is_empty()
        );
        assert!(engine.get_state::<LedgerTransactionStates>().is_none());
    }

    #[test]
    fn preloaded_constructor_retains_shared_states_cache_and_skips_registration() {
        let snapshot = Arc::new(DataCache::new(false));
        let (reader, resolver) = ports(&snapshot);
        let native = Arc::new(MetadataNative {
            hash: UInt160::from_bytes(&[0x41; 20]).expect("hash"),
            metadata_reads: AtomicUsize::new(0),
        });
        let state = Arc::new(
            native
                .contract_state(&ProtocolSettings::default(), 0)
                .unwrap(),
        );
        native.metadata_reads.store(0, Ordering::SeqCst);
        let cache = Arc::new(Mutex::new(NativeContractsCache::default()));
        let mut registry = NativeRegistry::empty();
        registry.register(native.clone());
        let engine = ApplicationEngine::new_with_preloaded_native_shared(
            TriggerType::Application,
            None,
            Arc::clone(&snapshot),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            HashMap::from([(native.hash, Arc::clone(&state))]),
            Arc::clone(&cache),
            None,
            services(registry, reader, resolver),
        )
        .expect("preloaded injected engine");
        assert_eq!(native.metadata_reads.load(Ordering::SeqCst), 0);
        assert!(Arc::ptr_eq(
            engine.contracts.get(&native.hash).unwrap(),
            &state
        ));
        assert!(Arc::ptr_eq(&engine.native_contract_cache_handle(), &cache));
        assert!(Arc::ptr_eq(&engine.snapshot_cache(), &snapshot));
        assert_eq!(engine.fee_consumed(), 0);
        assert_eq!(engine.gas_consumed(), 0);
    }

    struct RecordingObserver(Mutex<Vec<(usize, usize)>>);

    impl EngineEventObserver for RecordingObserver {
        fn notify_application_log(&self, engine: &ApplicationEngine, event: &LogEventArgs) {
            assert_eq!(event.message, "injected log");
            self.0
                .lock()
                .push((engine.logs().len(), engine.notifications().len()));
        }

        fn notify_application_notify(&self, engine: &ApplicationEngine, event: &NotifyEventArgs) {
            assert_eq!(event.event_name, "InjectedNotify");
            self.0
                .lock()
                .push((engine.logs().len(), engine.notifications().len()));
        }
    }

    #[test]
    fn event_observer_is_called_before_retention_and_can_be_detached() {
        let snapshot = Arc::new(DataCache::new(false));
        let (reader, resolver) = ports(&snapshot);
        let mut engine = ApplicationEngine::new_with_shared_block(
            TriggerType::Application,
            None,
            snapshot,
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            None,
            services(NativeRegistry::empty(), reader, resolver),
        )
        .expect("injected engine");
        let observer = Arc::new(RecordingObserver(Mutex::new(Vec::new())));
        engine.set_event_observer(Some(observer.clone()));
        engine.push_log(LogEventArgs::new(
            Arc::new(HashContainer),
            UInt160::zero(),
            "injected log".to_string(),
        ));
        engine.push_notification(NotifyEventArgs::new_with_optional_container(
            None,
            UInt160::zero(),
            "InjectedNotify".to_string(),
            Vec::new(),
        ));
        assert_eq!(*observer.0.lock(), vec![(0, 0), (1, 0)]);
        assert_eq!(engine.logs().len(), 1);
        assert_eq!(engine.notifications().len(), 1);
        engine.set_event_observer(None);
        engine.push_log(LogEventArgs::new(
            Arc::new(HashContainer),
            UInt160::zero(),
            "detached log".to_string(),
        ));
        assert_eq!(observer.0.lock().len(), 2);
        assert_eq!(engine.logs().len(), 2);
    }

    struct ChainQueries {
        height: u32,
        hash: UInt256,
        reads: AtomicUsize,
    }

    impl NativeChainQueries for ChainQueries {
        fn current_block_index(&self) -> u32 {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.height
        }

        fn block_hash_at(&self, index: u32) -> Option<UInt256> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            (index == self.height).then_some(self.hash)
        }
    }

    fn query_engine() -> ApplicationEngine {
        let snapshot = Arc::new(DataCache::new(false));
        let (reader, resolver) = ports(&snapshot);
        ApplicationEngine::new_with_shared_block(
            TriggerType::Application,
            None,
            snapshot,
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            None,
            services(NativeRegistry::empty(), reader, resolver),
        )
        .expect("query engine")
    }

    #[test]
    fn chain_queries_are_engine_instance_isolated_and_detachable() {
        let mut first = query_engine();
        let mut second = query_engine();
        assert!(first.chain_queries().is_none());
        assert!(second.chain_queries().is_none());
        let first_queries = Arc::new(ChainQueries {
            height: 7,
            hash: UInt256::from_bytes(&[7; 32]).unwrap(),
            reads: AtomicUsize::new(0),
        });
        let second_queries = Arc::new(ChainQueries {
            height: 19,
            hash: UInt256::from_bytes(&[19; 32]).unwrap(),
            reads: AtomicUsize::new(0),
        });
        first.attach_chain_queries(first_queries.clone());
        second.attach_chain_queries(second_queries.clone());
        let first_handle = first.chain_queries().unwrap();
        let second_handle = second.chain_queries().unwrap();
        assert_eq!(first_handle.current_block_index(), 7);
        assert_eq!(first_handle.block_hash_at(7), Some(first_queries.hash));
        assert_eq!(second_queries.reads.load(Ordering::SeqCst), 0);
        assert_eq!(second_handle.current_block_index(), 19);
        assert_eq!(second_handle.block_hash_at(19), Some(second_queries.hash));
        first.detach_chain_queries();
        assert!(first.chain_queries().is_none());
        assert!(second.chain_queries().is_some());
        assert_eq!(first_handle.current_block_index(), 7);
        assert_eq!(second.chain_queries().unwrap().current_block_index(), 19);
    }

    #[test]
    fn successful_snapshot_height_reads_do_not_touch_attached_queries() {
        let mut engine = query_engine();
        let queries = Arc::new(ChainQueries {
            height: 91,
            hash: UInt256::zero(),
            reads: AtomicUsize::new(0),
        });
        engine.attach_chain_queries(queries.clone());
        assert_eq!(engine.current_block_index(), 0);
        assert_eq!(queries.reads.load(Ordering::SeqCst), 0);
    }
}
