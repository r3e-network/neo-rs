//! Read-write set integration tests for speculative parallel execution.
//!
//! Every test below drives the real application engine: a "transaction" is a
//! script executed on its own private overlay of a shared base snapshot, which
//! is how a speculative scheduler runs transactions before it validates them.
//! The recorded [`RwSetTracker`] is then handed to the conflict checks a
//! validator would use, and the tests assert the accept/reject decision.

use std::sync::Arc;

use neo_primitives::{CallFlags, TriggerType, UInt160, Verifiable};
use neo_protocol::ProtocolSettings;
use neo_protocol::payloads::Transaction;
use neo_protocol::signer::Signer;
use neo_smart_contract::application_engine::{
    ApplicationEngine, EngineNativeServices, NativeRegistry, NativeStateReader, TEST_MODE_GAS,
    WitnessHashResolver,
};
use neo_smart_contract::storage_context::StorageContext;
use neo_smart_contract::{ContractState, CoreResult};
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use neo_vm::{OpCode, RwKey, RwSetTracker, ScriptBuilder, VmState};

/// The contract id every script below reads and writes.
const CONTRACT_ID: i32 = 7;

/// Native state reads are irrelevant to the scenarios below.
struct NoopStateReader;

impl NativeStateReader for NoopStateReader {
    fn contract(
        &self,
        _snapshot: &DataCache,
        _hash: &UInt160,
    ) -> CoreResult<Option<ContractState>> {
        Ok(None)
    }

    fn current_index(&self, _snapshot: &DataCache) -> CoreResult<u32> {
        Ok(0)
    }

    fn is_blocked(&self, _snapshot: &DataCache, _hash: &UInt160) -> CoreResult<bool> {
        Ok(false)
    }

    fn whitelisted_fee(
        &self,
        _snapshot: &DataCache,
        _hash: &UInt160,
        _method: &str,
        _param_count: u32,
    ) -> CoreResult<Option<i64>> {
        Ok(None)
    }

    fn oracle_original_signers(
        &self,
        _snapshot: &DataCache,
        _id: u64,
    ) -> CoreResult<Option<Vec<Signer>>> {
        Ok(None)
    }
}

/// Verifying-hash resolution is irrelevant to the scenarios below.
struct NoopResolver;

impl WitnessHashResolver for NoopResolver {
    fn resolve(&self, _verifiable: &dyn Verifiable, _snapshot: &DataCache) -> Vec<UInt160> {
        Vec::new()
    }
}

fn native_services() -> EngineNativeServices {
    EngineNativeServices {
        registry: NativeRegistry::empty(),
        state_reader: Arc::new(NoopStateReader),
        witness_hash_resolver: Arc::new(NoopResolver),
        policy_hash: UInt160::zero(),
        default_exec_fee_factor: 30,
        default_storage_price: 100_000,
    }
}

/// The shared base state every speculative transaction starts from.
fn base_snapshot(balance: i64) -> Arc<DataCache> {
    let snapshot = DataCache::new(false);
    for (suffix, value) in [
        (b"balance".as_slice(), balance.to_le_bytes().to_vec()),
        (b"p/a".as_slice(), b"1".to_vec()),
        (b"p/b".as_slice(), b"2".to_vec()),
    ] {
        snapshot.add(
            StorageKey::new(CONTRACT_ID, suffix.to_vec()),
            StorageItem::from_bytes(value),
        );
    }
    Arc::new(snapshot)
}

/// The read-write set key of one contract storage entry.
fn rw_key(suffix: &[u8]) -> RwKey {
    RwKey::new(StorageKey::new(CONTRACT_ID, suffix.to_vec()).to_array())
}

fn storage_context() -> Vec<u8> {
    StorageContext::read_write(CONTRACT_ID).to_bytes().to_vec()
}

/// `System.Storage.Get(context, key)` discards the value.
fn emit_get(script: &mut ScriptBuilder, key: &[u8]) {
    script.emit_push(key);
    script.emit_push(&storage_context());
    script
        .emit_syscall("System.Storage.Get")
        .expect("System.Storage.Get syscall");
    script.emit_opcode(OpCode::DROP);
}

/// `System.Storage.Put(context, key, value)`.
fn emit_put(script: &mut ScriptBuilder, key: &[u8], value: &[u8]) {
    script.emit_push(value);
    script.emit_push(key);
    script.emit_push(&storage_context());
    script
        .emit_syscall("System.Storage.Put")
        .expect("System.Storage.Put syscall");
}

/// `System.Storage.Find(context, prefix, options)` discards the iterator.
fn emit_find(script: &mut ScriptBuilder, prefix: &[u8]) {
    script.emit_push_int(0);
    script.emit_push(prefix);
    script.emit_push(&storage_context());
    script
        .emit_syscall("System.Storage.Find")
        .expect("System.Storage.Find syscall");
    script.emit_opcode(OpCode::DROP);
}

fn finish(mut script: ScriptBuilder) -> Vec<u8> {
    script.emit_opcode(OpCode::RET);
    script.to_array()
}

fn read_script(key: &[u8]) -> Vec<u8> {
    let mut script = ScriptBuilder::new();
    emit_get(&mut script, key);
    finish(script)
}

fn write_script(key: &[u8], value: &[u8]) -> Vec<u8> {
    let mut script = ScriptBuilder::new();
    emit_put(&mut script, key, value);
    finish(script)
}

fn read_then_write_script(read: &[u8], write: &[u8], value: &[u8]) -> Vec<u8> {
    let mut script = ScriptBuilder::new();
    emit_get(&mut script, read);
    emit_put(&mut script, write, value);
    finish(script)
}

/// An untracked engine over its own overlay of `base`.
fn untracked_engine(
    base: &Arc<DataCache>,
    container: Option<Arc<dyn Verifiable>>,
) -> ApplicationEngine {
    ApplicationEngine::new(
        TriggerType::Application,
        container,
        Arc::new(base.clone_cache()),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
        native_services(),
    )
    .expect("application engine")
}

/// Runs `script` as one speculative transaction and returns its read-write set.
fn execute_tx(
    base: &Arc<DataCache>,
    generation: u64,
    container: Option<Arc<dyn Verifiable>>,
    script: Vec<u8>,
) -> RwSetTracker {
    let mut engine = untracked_engine(base, container);
    engine.enable_rw_set_tracking_at_generation(generation);
    assert!(engine.is_rw_set_tracking_enabled());
    engine
        .load_script(script, CallFlags::ALL, None)
        .expect("load script");
    assert_eq!(engine.execute_allow_fault(), VmState::HALT);
    engine.take_rw_set().expect("recorded read-write set")
}

fn execute(base: &Arc<DataCache>, script: Vec<u8>) -> RwSetTracker {
    execute_tx(base, 0, None, script)
}

#[test]
fn tracking_is_opt_in_outside_the_crate() {
    let base = base_snapshot(100);
    let mut engine = untracked_engine(&base, None);
    assert!(!engine.is_rw_set_tracking_enabled());

    engine
        .load_script(read_script(b"balance"), CallFlags::ALL, None)
        .expect("load script");
    assert_eq!(engine.execute_allow_fault(), VmState::HALT);
    // Nothing was allocated, so there is nothing to hand to a validator.
    assert!(engine.rw_set().is_none());
    assert!(engine.take_rw_set().is_none());
}

#[test]
fn two_transactions_extract_rw_sets_and_detect_conflicts() {
    let base = base_snapshot(100);
    // Both transactions read the same key but write disjoint keys, so they are
    // independent even though they overlap on the read side.
    let first = execute(&base, read_then_write_script(b"balance", b"tx1/out", b"1"));
    let second = execute(&base, read_then_write_script(b"balance", b"tx2/out", b"2"));

    assert_eq!(first.read_set().len(), 1);
    assert!(first.read(&rw_key(b"balance")));
    assert_eq!(first.write_set().len(), 1);
    assert!(first.wrote(&rw_key(b"tx1/out")));
    assert!(!first.has_absent_reads());

    assert!(second.read(&rw_key(b"balance")));
    assert!(second.wrote(&rw_key(b"tx2/out")));

    assert!(!first.has_conflict(&second));
    assert!(!second.has_conflict(&first));
}

#[test]
fn write_after_read_conflict_is_detected() {
    let base = base_snapshot(100);
    let reader = execute(&base, read_then_write_script(b"balance", b"tx1/out", b"1"));
    let writer = execute(&base, write_script(b"balance", b"0"));

    assert_eq!(writer.read_set().len(), 0);
    assert!(writer.wrote(&rw_key(b"balance")));
    assert!(reader.has_conflict(&writer));
    assert!(writer.has_conflict(&reader));

    // Ordered validation: the writer commits after the reader observed the old
    // balance, so the reader's observation is stale and it must re-execute.
    let mut committed = RwSetTracker::new();
    committed.merge_committed(&writer);
    assert_eq!(reader.stale_read_keys(&committed), vec![rw_key(b"balance")]);
    assert!(reader.has_stale_reads(&committed));

    // A disjoint writer leaves the read valid.
    let mut unrelated = RwSetTracker::new();
    unrelated.merge_committed(&execute(&base, write_script(b"tx9/out", b"9")));
    assert!(!reader.has_stale_reads(&unrelated));
}

#[test]
fn read_that_observed_a_commit_is_not_stale() {
    let base = base_snapshot(100);
    // `writer` is the transaction that commits first.
    let writer = execute(&base, write_script(b"balance", b"0"));
    let mut committed = RwSetTracker::new();
    committed.merge_committed(&writer);
    let generation = committed.write_clock();

    // The next transaction observes the committed state, so the engine tracker
    // is stamped on the committed clock rather than from scratch.
    let after_commit_base = base_snapshot(0);
    let reader = execute_tx(
        &after_commit_base,
        generation,
        None,
        read_then_write_script(b"balance", b"tx1/out", b"1"),
    );

    assert_eq!(reader.generation(), generation);
    assert!(reader.read(&rw_key(b"balance")));
    assert!(reader.stale_read_keys(&committed).is_empty());
    assert!(!reader.has_stale_reads(&committed));
    // The structural check still reports the shared key, which is why ordered
    // validation uses the version-aware check.
    assert!(reader.has_conflict(&writer));
}

#[test]
fn range_reads_conflict_only_inside_the_scanned_prefix() {
    let base = base_snapshot(100);
    let mut script = ScriptBuilder::new();
    emit_find(&mut script, b"p/");
    let scanner = execute(&base, finish(script));

    assert!(scanner.has_read_prefixes());
    // The entries the scan returned are exact reads as well.
    assert!(scanner.read(&rw_key(b"p/a")));
    assert!(scanner.read(&rw_key(b"p/b")));

    let inside = execute(&base, write_script(b"p/new", b"1"));
    assert!(scanner.has_conflict(&inside));
    assert!(inside.has_conflict(&scanner));

    let outside = execute(&base, write_script(b"q/new", b"1"));
    assert!(!scanner.has_conflict(&outside));
}

#[test]
fn absent_reads_are_dependencies() {
    let base = base_snapshot(100);
    let reader = execute(&base, read_script(b"p/absent"));

    assert!(reader.read(&rw_key(b"p/absent")));
    assert!(reader.has_absent_reads());
    assert!(reader.absent_read_set().contains(&rw_key(b"p/absent")));

    // Creating the key later invalidates the absence observation.
    let creator = execute(&base, write_script(b"p/absent", b"1"));
    assert!(reader.has_conflict(&creator));
    assert!(creator.has_conflict(&reader));

    let mut committed = RwSetTracker::new();
    committed.merge_committed(&creator);
    assert_eq!(
        reader.stale_read_keys(&committed),
        vec![rw_key(b"p/absent")]
    );
}

#[test]
fn events_are_recorded_as_ordering_barriers() {
    let base = base_snapshot(100);
    let mut script = ScriptBuilder::new();
    script.emit_push(b"payload");
    script
        .emit_syscall("System.Runtime.Log")
        .expect("System.Runtime.Log syscall");
    // Logging requires a script container, as on chain.
    let container: Arc<dyn Verifiable> = Arc::new(Transaction::new());
    let set = execute_tx(&base, 0, Some(container), finish(script));

    assert!(set.has_events());
    assert_eq!(set.log_events.len(), 1);
    assert!(set.notify_events.is_empty());
    // Events are observable outside the write set, so they never act as keys.
    assert!(!set.has_reads() && !set.has_writes());
    assert!(!set.has_conflict(&RwSetTracker::new()));
}
