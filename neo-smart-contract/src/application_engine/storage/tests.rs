use super::*;
use crate::error::CoreResult;
use neo_protocol::signer::Signer;
use neo_vm::{RwKey, RwSetTracker, ScriptBuilder, VmState};

// Engine-owned no-op native service doubles. The engine layer must not
// depend on concrete natives, so these tests inject minimal trait objects
// rather than the composition crate's real reader. The two tests below
// execute storage syscalls against a contract injected via
// `put_contract_cache` and never trigger a native/contract read, so the
// empty implementations are sufficient and introduce no reverse edge.
struct NoopStateReader;
impl NativeStateReader for NoopStateReader {
    fn contract(
        &self,
        _snapshot: &PersistenceDataCache,
        _hash: &UInt160,
    ) -> CoreResult<Option<ContractState>> {
        Ok(None)
    }
    fn current_index(&self, _snapshot: &PersistenceDataCache) -> CoreResult<u32> {
        Ok(0)
    }
    fn is_blocked(&self, _snapshot: &PersistenceDataCache, _hash: &UInt160) -> CoreResult<bool> {
        Ok(false)
    }
    fn whitelisted_fee(
        &self,
        _snapshot: &PersistenceDataCache,
        _hash: &UInt160,
        _method: &str,
        _param_count: u32,
    ) -> CoreResult<Option<i64>> {
        Ok(None)
    }
    fn oracle_original_signers(
        &self,
        _snapshot: &PersistenceDataCache,
        _id: u64,
    ) -> CoreResult<Option<Vec<Signer>>> {
        Ok(None)
    }
}
struct NoopResolver;
impl WitnessHashResolver for NoopResolver {
    fn resolve(
        &self,
        _verifiable: &dyn neo_primitives::Verifiable,
        _snapshot: &PersistenceDataCache,
    ) -> Vec<UInt160> {
        Vec::new()
    }
}
fn test_services() -> EngineNativeServices {
    EngineNativeServices {
        registry: NativeRegistry::empty(),
        state_reader: Arc::new(NoopStateReader),
        witness_hash_resolver: Arc::new(NoopResolver),
        policy_hash: UInt160::zero(),
        default_exec_fee_factor: 30,
        default_storage_price: 100_000,
    }
}

fn contract_engine(
    snapshot: Arc<PersistenceDataCache>,
    script: Vec<u8>,
) -> (ApplicationEngine, UInt160) {
    contract_engine_with_container(snapshot, script, None)
}

fn contract_engine_with_container(
    snapshot: Arc<PersistenceDataCache>,
    script: Vec<u8>,
    container: Option<Arc<dyn Verifiable>>,
) -> (ApplicationEngine, UInt160) {
    let hash = UInt160::from_bytes(&[0x42; 20]).expect("contract hash");
    let mut contract = ContractState::default();
    contract.id = 42;
    contract.hash = hash;
    // `ProtocolSettings::default()` normalizes Huyao (the last
    // configuration-managed hardfork) to genesis, which routes Faun through
    // the policy table; the Noop state reader has no such table, so keep
    // Huyao ahead of this block and enable Faun from the configuration
    // (see policy_contract tests for the same pattern).
    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfFaun, 0);
    settings.hardforks.insert(Hardfork::HfHuyao, 1_000_000);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        container,
        snapshot,
        None,
        settings,
        TEST_MODE_GAS,
        None,
        test_services(),
    )
    .expect("application engine");
    assert!(engine.is_hardfork_enabled(Hardfork::HfFaun));
    engine.put_contract_cache(hash, contract);
    engine
        .load_script(script, CallFlags::ALL, Some(hash))
        .expect("load contract script");
    (engine, hash)
}

#[test]
fn local_put_pops_key_before_value() {
    let snapshot = Arc::new(PersistenceDataCache::new(false));
    let mut script = ScriptBuilder::new();
    // C# scripts push value first, then key; PutLocal pops key, then value.
    script.emit_push(b"value");
    script.emit_push(b"key");
    script
        .emit_syscall("System.Storage.Local.Put")
        .expect("Local.Put syscall");
    script.emit_opcode(OpCode::RET);
    let (mut engine, _) = contract_engine(Arc::clone(&snapshot), script.to_array());

    assert_eq!(engine.execute_allow_fault(), VmState::HALT);
    let stored = snapshot
        .get(&StorageKey::new(42, b"key".to_vec()))
        .expect("stored value");
    assert_eq!(stored.value_bytes().as_ref(), b"value");
    assert!(
        snapshot
            .get(&StorageKey::new(42, b"value".to_vec()))
            .is_none()
    );
}

#[test]
fn local_find_pops_prefix_before_options() {
    let snapshot = PersistenceDataCache::new(false);
    snapshot.add(
        StorageKey::new(42, b"ab1".to_vec()),
        StorageItem::from_bytes(b"one".to_vec()),
    );
    snapshot.add(
        StorageKey::new(42, b"ab2".to_vec()),
        StorageItem::from_bytes(b"two".to_vec()),
    );
    let snapshot = Arc::new(snapshot);
    let mut script = ScriptBuilder::new();
    // FindLocal pops prefix, then options, just like Find after its context.
    script.emit_push_int(0);
    script.emit_push(b"ab");
    script
        .emit_syscall("System.Storage.Local.Find")
        .expect("Local.Find syscall");
    script.emit_opcode(OpCode::DUP);
    script
        .emit_syscall("System.Iterator.Next")
        .expect("Iterator.Next syscall");
    script.emit_opcode(OpCode::DROP);
    script
        .emit_syscall("System.Iterator.Value")
        .expect("Iterator.Value syscall");
    script.emit_opcode(OpCode::RET);
    let (mut engine, _) = contract_engine(snapshot, script.to_array());

    assert_eq!(engine.execute_allow_fault(), VmState::HALT);
    let result = engine.result_stack().peek(0).expect("iterator result");
    let entry = result.as_array().expect("iterator entry");
    assert_eq!(entry[0].as_bytes().expect("entry key"), b"ab1");
    assert_eq!(entry[1].as_bytes().expect("entry value"), b"one");
}

// ---- System.Storage.FindWithStart (HF_Huyao) ---------------------------

/// `contract_engine_with_container` with `HF_Huyao` active at height 1: over
/// the empty snapshot the persisting index is 1, so the hardfork resolves
/// through ProtocolSettings without consulting the (empty) on-chain Policy
/// table (see the std_lib strLen tests for the same trick).
fn huyao_contract_engine(
    snapshot: Arc<PersistenceDataCache>,
    script: Vec<u8>,
) -> (ApplicationEngine, UInt160) {
    let hash = UInt160::from_bytes(&[0x42; 20]).expect("contract hash");
    let mut contract = ContractState::default();
    contract.id = 42;
    contract.hash = hash;
    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfFaun, 0);
    settings.hardforks.insert(Hardfork::HfHuyao, 1);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        settings,
        TEST_MODE_GAS,
        None,
        test_services(),
    )
    .expect("application engine");
    assert!(engine.is_hardfork_enabled(Hardfork::HfHuyao));
    engine.put_contract_cache(hash, contract);
    engine
        .load_script(script, CallFlags::ALL, Some(hash))
        .expect("load contract script");
    (engine, hash)
}

/// Seeds contract 42 storage with suffixes spanning both sides of the
/// search starts used by the tests below.
fn seed_find_entries(snapshot: &PersistenceDataCache) {
    for (suffix, value) in [
        ("ab0", "zero"),
        ("ab1", "one"),
        ("ab2", "two"),
        ("ac1", "other"),
        ("b1", "unrelated"),
    ] {
        snapshot.add(
            StorageKey::new(42, suffix.as_bytes().to_vec()),
            StorageItem::from_bytes(value.as_bytes().to_vec()),
        );
    }
}

/// Emits one iterator round: `DUP Next DROP DUP Value`, leaving the entry
/// pushed above the iterator.
fn emit_next_value(script: &mut ScriptBuilder) {
    script.emit_opcode(OpCode::DUP);
    script
        .emit_syscall("System.Iterator.Next")
        .expect("Iterator.Next syscall");
    script.emit_opcode(OpCode::DROP);
    script.emit_opcode(OpCode::DUP);
    script
        .emit_syscall("System.Iterator.Value")
        .expect("Iterator.Value syscall");
}

#[test]
fn find_with_start_starts_at_inclusive_suffix() {
    let snapshot = PersistenceDataCache::new(false);
    seed_find_entries(&snapshot);
    let snapshot = Arc::new(snapshot);
    let context = context_bytes(CONTRACT_ID);
    let mut script = ScriptBuilder::new();
    // FindWithStart(context, prefix, start, options) pops context, prefix,
    // start, options; the script pushes them in declaration order.
    script.emit_push_int(0); // FindOptions::None
    script.emit_push(b"1"); // start
    script.emit_push(b"ab"); // prefix
    script.emit_push(&context);
    script
        .emit_syscall("System.Storage.FindWithStart")
        .expect("FindWithStart syscall");
    emit_next_value(&mut script);
    // Round two starts from [entry1, iterator], so swap the iterator back
    // to the top before repeating the round.
    script.emit_opcode(OpCode::SWAP);
    emit_next_value(&mut script);
    script.emit_opcode(OpCode::RET);
    let (mut engine, _) = huyao_contract_engine(Arc::clone(&snapshot), script.to_array());

    assert_eq!(engine.execute_allow_fault(), VmState::HALT);
    // C# seeks from `prefixKey + start` inclusively, so `ab1` itself is
    // the first entry and the plain `Find` head `ab0` is skipped.
    let first = engine
        .result_stack()
        .peek(2)
        .expect("first entry")
        .as_array()
        .expect("iterator entry");
    assert_eq!(first[0].as_bytes().expect("first key"), b"ab1");
    assert_eq!(first[1].as_bytes().expect("first value"), b"one");
    let second = engine
        .result_stack()
        .peek(0)
        .expect("second entry")
        .as_array()
        .expect("iterator entry");
    assert_eq!(second[0].as_bytes().expect("second key"), b"ab2");
    assert_eq!(second[1].as_bytes().expect("second value"), b"two");
}

#[test]
fn find_with_start_backwards_is_inclusive_and_descending() {
    let snapshot = PersistenceDataCache::new(false);
    seed_find_entries(&snapshot);
    let snapshot = Arc::new(snapshot);
    let context = context_bytes(CONTRACT_ID);
    let mut script = ScriptBuilder::new();
    script.emit_push_int(i64::from(FindOptions::Backwards.bits()));
    script.emit_push(b"1");
    script.emit_push(b"ab");
    script.emit_push(&context);
    script
        .emit_syscall("System.Storage.FindWithStart")
        .expect("FindWithStart syscall");
    emit_next_value(&mut script);
    script.emit_opcode(OpCode::SWAP);
    emit_next_value(&mut script);
    script.emit_opcode(OpCode::RET);
    let (mut engine, _) = huyao_contract_engine(Arc::clone(&snapshot), script.to_array());

    assert_eq!(engine.execute_allow_fault(), VmState::HALT);
    // Backwards seeks from `ab1` downward: the start entry is included and
    // the order descends (`ab1`, `ab0`); `ab2` and `ac1` are above it.
    let first = engine
        .result_stack()
        .peek(2)
        .expect("first entry")
        .as_array()
        .expect("iterator entry");
    assert_eq!(first[0].as_bytes().expect("first key"), b"ab1");
    let second = engine
        .result_stack()
        .peek(0)
        .expect("second entry")
        .as_array()
        .expect("iterator entry");
    assert_eq!(second[0].as_bytes().expect("second key"), b"ab0");
}

#[test]
fn find_with_start_strips_prefix_and_returns_keys_only() {
    let snapshot = PersistenceDataCache::new(false);
    seed_find_entries(&snapshot);
    let snapshot = Arc::new(snapshot);
    let context = context_bytes(CONTRACT_ID);
    let mut script = ScriptBuilder::new();
    let options = (FindOptions::KeysOnly | FindOptions::RemovePrefix).bits();
    script.emit_push_int(i64::from(options));
    script.emit_push(b"1");
    script.emit_push(b"ab");
    script.emit_push(&context);
    script
        .emit_syscall("System.Storage.FindWithStart")
        .expect("FindWithStart syscall");
    emit_next_value(&mut script);
    script.emit_opcode(OpCode::RET);
    let (mut engine, _) = huyao_contract_engine(Arc::clone(&snapshot), script.to_array());

    assert_eq!(engine.execute_allow_fault(), VmState::HALT);
    // The iterator carries `prefix.Length` so RemovePrefix strips `ab`.
    let key = engine
        .result_stack()
        .peek(0)
        .expect("first key")
        .as_bytes()
        .expect("key bytes");
    assert_eq!(key, b"1");
}

#[test]
fn find_local_with_start_uses_current_contract() {
    let snapshot = PersistenceDataCache::new(false);
    seed_find_entries(&snapshot);
    let snapshot = Arc::new(snapshot);
    let mut script = ScriptBuilder::new();
    // FindLocalWithStart(prefix, start, options) pops prefix, start,
    // options, so the script pushes options first and prefix last.
    script.emit_push_int(0);
    script.emit_push(b"1");
    script.emit_push(b"ab");
    script
        .emit_syscall("System.Storage.Local.FindWithStart")
        .expect("Local.FindWithStart syscall");
    emit_next_value(&mut script);
    script.emit_opcode(OpCode::RET);
    let (mut engine, _) = huyao_contract_engine(Arc::clone(&snapshot), script.to_array());

    assert_eq!(engine.execute_allow_fault(), VmState::HALT);
    let entry = engine
        .result_stack()
        .peek(0)
        .expect("iterator result")
        .as_array()
        .expect("iterator entry");
    assert_eq!(entry[0].as_bytes().expect("entry key"), b"ab1");
    assert_eq!(entry[1].as_bytes().expect("entry value"), b"one");
}

#[test]
fn find_with_start_faults_before_huyao() {
    let snapshot = PersistenceDataCache::new(false);
    seed_find_entries(&snapshot);
    let snapshot = Arc::new(snapshot);
    let context = context_bytes(CONTRACT_ID);
    let mut script = ScriptBuilder::new();
    script.emit_push_int(0);
    script.emit_push(b"1");
    script.emit_push(b"ab");
    script.emit_push(&context);
    script
        .emit_syscall("System.Storage.FindWithStart")
        .expect("FindWithStart syscall");
    script.emit_opcode(OpCode::RET);
    // `contract_engine` keeps `HF_Huyao` at height 1_000_000, so the
    // descriptor is absent and the unknown syscall must fault (C# fails
    // with an unregistered-method error before `HF_Huyao`).
    let (mut engine, _) = contract_engine(Arc::clone(&snapshot), script.to_array());

    assert_eq!(engine.execute_allow_fault(), VmState::FAULT);
}

// ---- read-write set instrumentation -----------------------------------

const CONTRACT_ID: i32 = 42;

fn context_bytes(contract_id: i32) -> Vec<u8> {
    StorageContext::read_write(contract_id).to_bytes().to_vec()
}

/// The read-write set key of a contract storage entry.
fn rw_key(contract_id: i32, suffix: &[u8]) -> RwKey {
    RwKey::new(StorageKey::new(contract_id, suffix.to_vec()).to_array())
}

/// Reads `read_key` (which is absent) and writes `write_key`.
fn read_then_write_script(
    contract_id: i32,
    read_key: &[u8],
    write_key: &[u8],
    value: &[u8],
) -> Vec<u8> {
    let context = context_bytes(contract_id);
    let mut script = ScriptBuilder::new();
    // System.Storage.Get(context, key) pops the context first, then the key.
    script.emit_push(read_key);
    script.emit_push(&context);
    script
        .emit_syscall("System.Storage.Get")
        .expect("Get syscall");
    script.emit_opcode(OpCode::DROP);
    // System.Storage.Put(context, key, value) pops value last.
    script.emit_push(value);
    script.emit_push(write_key);
    script.emit_push(&context);
    script
        .emit_syscall("System.Storage.Put")
        .expect("Put syscall");
    script.emit_opcode(OpCode::RET);
    script.to_array()
}

/// Builds an engine for `script` with read-write set recording enabled.
fn tracked_engine(snapshot: &Arc<PersistenceDataCache>, script: Vec<u8>) -> ApplicationEngine {
    tracked_engine_with_container(snapshot, script, None)
}

fn tracked_engine_with_container(
    snapshot: &Arc<PersistenceDataCache>,
    script: Vec<u8>,
    container: Option<Arc<dyn Verifiable>>,
) -> ApplicationEngine {
    let (mut engine, _) = contract_engine_with_container(Arc::clone(snapshot), script, container);
    engine.enable_rw_set_tracking();
    assert!(engine.is_rw_set_tracking_enabled());
    engine
}

fn run_script(snapshot: &Arc<PersistenceDataCache>, script: Vec<u8>) -> RwSetTracker {
    let mut engine = tracked_engine(snapshot, script);
    assert_eq!(engine.execute_allow_fault(), VmState::HALT);
    engine.take_rw_set().expect("recorded read-write set")
}

#[test]
fn rw_set_tracking_is_off_by_default() {
    let snapshot = Arc::new(PersistenceDataCache::new(false));
    let script = read_then_write_script(CONTRACT_ID, b"missing", b"stored", b"value");
    let (mut engine, _) = contract_engine(Arc::clone(&snapshot), script);

    assert!(!engine.is_rw_set_tracking_enabled());
    assert_eq!(engine.execute_allow_fault(), VmState::HALT);
    assert!(engine.rw_set().is_none());
    assert!(engine.take_rw_set().is_none());
    // Disabled instrumentation must not change execution.
    assert!(
        snapshot
            .get(&StorageKey::new(CONTRACT_ID, b"stored".to_vec()))
            .is_some()
    );
}

#[test]
fn rw_set_records_reads_absent_reads_and_writes() {
    let snapshot = Arc::new(PersistenceDataCache::new(false));
    let script = read_then_write_script(CONTRACT_ID, b"missing", b"stored", b"value");
    let set = run_script(&snapshot, script);

    assert_eq!(set.read_set().len(), 1);
    assert!(set.read(&rw_key(CONTRACT_ID, b"missing")));
    assert!(
        set.absent_read_set()
            .contains(&rw_key(CONTRACT_ID, b"missing"))
    );
    assert_eq!(set.write_set().len(), 1);
    assert!(set.wrote(&rw_key(CONTRACT_ID, b"stored")));
    assert!(!set.has_read_prefixes());
    assert!(!set.has_events());
    assert_eq!(set.generation(), 0);
}

#[test]
fn rw_set_marks_present_reads_as_non_absent() {
    let snapshot = PersistenceDataCache::new(false);
    snapshot.add(
        StorageKey::new(CONTRACT_ID, b"present".to_vec()),
        StorageItem::from_bytes(b"value".to_vec()),
    );
    let snapshot = Arc::new(snapshot);
    let script = read_then_write_script(CONTRACT_ID, b"present", b"stored", b"value");
    let set = run_script(&snapshot, script);

    assert!(set.read(&rw_key(CONTRACT_ID, b"present")));
    assert!(!set.has_absent_reads());
}

#[test]
fn rw_sets_of_two_transactions_detect_conflicts() {
    let snapshot = Arc::new(PersistenceDataCache::new(false));
    let set_a = run_script(
        &snapshot,
        read_then_write_script(CONTRACT_ID, b"shared", b"a-only", b"1"),
    );
    let set_b = run_script(
        &snapshot,
        read_then_write_script(CONTRACT_ID, b"shared", b"b-only", b"2"),
    );

    // Both read `shared`, but neither writes it: they may run in parallel.
    assert!(set_a.read(&rw_key(CONTRACT_ID, b"shared")));
    assert!(set_b.read(&rw_key(CONTRACT_ID, b"shared")));
    assert!(!set_a.has_conflict(&set_b));
    assert!(!set_b.has_conflict(&set_a));

    // A third transaction writing the shared key invalidates both reads.
    let set_c = run_script(
        &snapshot,
        read_then_write_script(CONTRACT_ID, b"never-read", b"shared", b"3"),
    );
    assert!(set_a.has_conflict(&set_c));
    assert!(set_b.has_conflict(&set_c));
    assert!(set_c.has_conflict(&set_a));

    // Ordered validation: `c` committed after `a` read, so `a` is stale.
    let mut committed = RwSetTracker::new();
    committed.merge_committed(&set_c);
    assert_eq!(
        set_a.stale_read_keys(&committed),
        vec![rw_key(CONTRACT_ID, b"shared")]
    );
    assert!(set_a.has_stale_reads(&committed));
}

#[test]
fn rw_sets_detect_write_write_conflicts() {
    let snapshot = Arc::new(PersistenceDataCache::new(false));
    let first = run_script(
        &snapshot,
        read_then_write_script(CONTRACT_ID, b"k1", b"same", b"1"),
    );
    let second = run_script(
        &snapshot,
        read_then_write_script(CONTRACT_ID, b"k2", b"same", b"2"),
    );

    assert!(first.has_conflict(&second));
    assert!(second.has_conflict(&first));
}

#[test]
fn rw_set_records_find_scans_as_prefix_reads() {
    let snapshot = PersistenceDataCache::new(false);
    snapshot.add(
        StorageKey::new(CONTRACT_ID, b"p/x".to_vec()),
        StorageItem::from_bytes(b"1".to_vec()),
    );
    snapshot.add(
        StorageKey::new(CONTRACT_ID, b"p/y".to_vec()),
        StorageItem::from_bytes(b"2".to_vec()),
    );
    let snapshot = Arc::new(snapshot);

    let context = context_bytes(CONTRACT_ID);
    let mut script = ScriptBuilder::new();
    // System.Storage.Find(context, prefix, options) pops context, prefix, options.
    script.emit_push_int(0);
    script.emit_push(b"p/");
    script.emit_push(&context);
    script
        .emit_syscall("System.Storage.Find")
        .expect("Find syscall");
    script.emit_opcode(OpCode::DROP);
    script.emit_opcode(OpCode::RET);

    let set = run_script(&snapshot, script.to_array());

    assert!(set.has_read_prefixes());
    assert!(set.read(&rw_key(CONTRACT_ID, b"p/x")));
    assert!(set.read(&rw_key(CONTRACT_ID, b"p/y")));
    assert!(!set.has_writes());

    // A key created inside the scanned range afterwards conflicts.
    let mut later = RwSetTracker::new();
    later.record_write(rw_key(CONTRACT_ID, b"p/z"));
    assert!(set.has_conflict(&later));

    // A write outside the range does not.
    let mut unrelated = RwSetTracker::new();
    unrelated.record_write(rw_key(CONTRACT_ID, b"q/z"));
    assert!(!set.has_conflict(&unrelated));
}

#[test]
fn rw_set_records_notifications_and_logs() {
    let snapshot = Arc::new(PersistenceDataCache::new(false));
    let script_hash = UInt160::from_bytes(&[0x42; 20]).expect("contract hash");
    let mut script = ScriptBuilder::new();
    script.emit_push(b"payload");
    script
        .emit_syscall("System.Runtime.Log")
        .expect("Log syscall");
    script.emit_opcode(OpCode::RET);

    // Logging requires a script container, as on chain.
    let container: Arc<dyn Verifiable> = Arc::new(Transaction::new());
    let mut engine = tracked_engine_with_container(&snapshot, script.to_array(), Some(container));
    assert_eq!(engine.execute_allow_fault(), VmState::HALT);
    // Native contracts emit notifications through the host helper; dynamic
    // scripts may not, because they have no deployed contract context.
    engine
        .emit_notification(&script_hash, "Transfer", &[])
        .expect("notification");

    let set = engine.take_rw_set().expect("recorded read-write set");
    assert!(set.has_events());
    assert_eq!(set.log_events.len(), 1);
    assert_eq!(set.notify_events.len(), 1);
    // Events are ordering barriers, not conflict keys.
    assert!(!set.has_reads() && !set.has_writes());
}

#[test]
fn disabling_rw_set_tracking_drops_recorded_data() {
    let snapshot = Arc::new(PersistenceDataCache::new(false));
    let script = read_then_write_script(CONTRACT_ID, b"missing", b"stored", b"value");
    let (mut engine, _) = contract_engine(Arc::clone(&snapshot), script);

    engine.enable_rw_set_tracking();
    assert_eq!(engine.execute_allow_fault(), VmState::HALT);
    assert!(engine.rw_set().is_some());

    engine.disable_rw_set_tracking();
    assert!(!engine.is_rw_set_tracking_enabled());
    assert!(engine.rw_set().is_none());
    assert!(engine.take_rw_set().is_none());
}
