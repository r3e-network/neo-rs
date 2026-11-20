//! ApplicationLogs service for capturing execution logs and serving RPC queries.

use neo_ledger::ApplicationExecuted;
use neo_primitives::TriggerType;
use neo_primitives::UInt256;
use neo_primitives::panic_message;
use neo_protocol::execution::Block as ExecutionBlock;
use neo_rpc_client::serialization::stack_item_json::{
    stack_item_rpc_json, stack_items_rpc_json_per_item,
};
use neo_storage::persistence::{DataCache, Store, StoreSnapshot};
use neo_system::NeoSystem;
use neo_system::i_event_handlers::{CommittedHandler, CommittingHandler};
use neo_vm::NotifyEventArgs;
use neo_vm::StackItem;
use neo_vm::VmState as VMState;
use parking_lot::Mutex;
use serde_json::{Map, Value};
use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{error, warn};

use super::ApplicationLogsSettings;

/// Serialized bytes an import session may hold before it commits on its own.
///
/// One checkpoint interval can cover tens of thousands of blocks, and a
/// transaction-bearing block can carry a large log. The cap bounds the memory a
/// session pins between two checkpoints without changing what is stored: an
/// automatic commit writes exactly the records the next checkpoint would have
/// written.
pub const DEFAULT_ACCEPTED_BYTE_CAP: u64 = 64 * 1024 * 1024;

/// One application-log record staged inside a persistence unit.
///
/// The key and JSON value are exactly what a standalone block would have put
/// into the logs snapshot, so promotion cannot change the stored bytes.
struct StagedLog {
    key: Vec<u8>,
    value: Vec<u8>,
}

impl StagedLog {
    /// Serialized size this record occupies in the logs database.
    fn byte_len(&self) -> u64 {
        u64::try_from(self.key.len().saturating_add(self.value.len())).unwrap_or(u64::MAX)
    }
}

/// Records held by the persistence unit that is currently open.
///
/// `staged` holds records written by a committing callback whose committed
/// callback has not fired yet. `committed` holds records whose committed
/// callback already fired inside this unit. Neither list has reached the logs
/// database, so a chain-commit failure can still discard both.
///
/// `watermark` is the newest block the unit covers. It travels with the records
/// so a commit can report the exact height its bytes make durable.
#[derive(Default)]
struct OpenUnit {
    staged: Vec<StagedLog>,
    committed: Vec<StagedLog>,
    watermark: Option<(u32, UInt256)>,
}

/// Outcome of an import-scoped persistence-unit lifecycle call.
///
/// The lifecycle is defensive by construction: a repeated `begin` keeps the
/// unit that is already open, and `finish`/`abort` without an open unit change
/// nothing. Callers can therefore react to misuse instead of unwinding a panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistenceUnitOutcome {
    /// `begin` opened a new unit.
    Started,
    /// `finish` promoted this many staged records to accepted-pending.
    Finished {
        /// Records the finished unit handed to `flush_accepted`.
        records: usize,
    },
    /// `abort` discarded this many staged records.
    Aborted {
        /// Records dropped from the unit that was open.
        records: usize,
    },
    /// `begin` while a unit was already open; the open unit is untouched.
    AlreadyOpen,
    /// `finish`/`abort` without an open unit; no record was changed.
    NotOpen,
}

/// Outcome of one accepted-record commit.
///
/// `watermark` is the block watermark the committed bytes cover. The bytes sit
/// in the backend, not on disk, until
/// [`ApplicationLogsService::try_flush_durable`] confirms the flush, so a caller
/// must not certify a checkpoint from this value alone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AcceptedFlush {
    /// Records written by this commit.
    pub records: u64,
    /// Serialized bytes those records occupy.
    pub bytes: u64,
    /// Highest block whose records a commit has written, if a block boundary was
    /// promoted with them. Durability requires the separate fallible flush.
    pub watermark: Option<(u32, UInt256)>,
}

/// Logs persistence telemetry of one import session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LogsFlushTelemetry {
    /// Commits that wrote at least one record.
    pub flushes: u64,
    /// Records those commits wrote.
    pub records: u64,
    /// Serialized bytes those commits wrote.
    pub bytes: u64,
    /// Records still waiting for a commit.
    pub pending_records: u64,
    /// Highest height whose records are durable.
    pub watermark: Option<(u32, UInt256)>,
}

/// Outcome of an import-session lifecycle call.
///
/// Like [`PersistenceUnitOutcome`], the lifecycle is defensive: a repeated
/// `begin` keeps the session that is already open, and `end` without a session
/// flushes nothing. Callers react to misuse instead of unwinding a panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportSessionOutcome {
    /// `begin` opened a session with a fresh watermark and flush counters.
    Started,
    /// `begin` while a session was already open; its state is untouched.
    AlreadyOpen,
    /// `end` committed the records still pending and left batching mode.
    Ended {
        /// Records the closing commit wrote.
        records: u64,
        /// Serialized bytes the closing commit wrote.
        bytes: u64,
    },
    /// `end` without an open session; nothing was written.
    NotOpen,
}

/// Records promoted by finished persistence units plus the import session state.
///
/// Online operation never promotes a record, so every field but `byte_cap`
/// keeps its default unless an import session is open.
struct AcceptedLogs {
    /// Records a finished unit promoted and no commit has written yet.
    records: Vec<StagedLog>,
    /// Serialized size of `records`.
    pending_bytes: u64,
    /// Newest block covered by `records`.
    pending_watermark: Option<(u32, UInt256)>,
    /// Newest block whose records a commit wrote but no durable flush confirmed.
    ///
    /// The import profile disables the write-ahead log, so bytes accepted by a
    /// commit are still lost on a crash until a durable flush succeeds. Keeping
    /// this stage separate from `durable_watermark` is what stops a checkpoint
    /// from certifying records that never reached disk.
    committed_watermark: Option<(u32, UInt256)>,
    /// Newest block whose records are durable in the logs database.
    durable_watermark: Option<(u32, UInt256)>,
    /// Whether [`ApplicationLogsService::begin_import_session`] is in effect.
    session_active: bool,
    /// Automatic commit threshold in serialized bytes; zero commits every unit.
    byte_cap: u64,
    /// Commits that wrote at least one record.
    flushes: u64,
    /// Records those commits wrote.
    flushed_records: u64,
    /// Serialized bytes those commits wrote.
    flushed_bytes: u64,
}

impl Default for AcceptedLogs {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            pending_bytes: 0,
            pending_watermark: None,
            committed_watermark: None,
            durable_watermark: None,
            session_active: false,
            byte_cap: DEFAULT_ACCEPTED_BYTE_CAP,
            flushes: 0,
            flushed_records: 0,
            flushed_bytes: 0,
        }
    }
}

impl AcceptedLogs {
    /// Promotes the records of a finished unit and returns their byte size.
    ///
    /// A unit's blocks are always newer than what is already pending, so its
    /// watermark replaces the pending one instead of being merged.
    fn push(&mut self, records: Vec<StagedLog>, watermark: Option<(u32, UInt256)>) -> u64 {
        let bytes = records.iter().map(StagedLog::byte_len).sum();
        self.pending_bytes = self.pending_bytes.saturating_add(bytes);
        self.records.extend(records);
        if watermark.is_some() {
            self.pending_watermark = watermark;
        }
        bytes
    }
}

/// ApplicationLogs storage and commit handler.
pub struct ApplicationLogsService {
    settings: ApplicationLogsSettings,
    store: Arc<dyn Store>,
    snapshot: Mutex<Option<Arc<dyn StoreSnapshot>>>,
    disabled: AtomicBool,
    /// Records promoted by finished units plus the import session state.
    accepted: Mutex<AcceptedLogs>,
    /// The import-scoped persistence unit, when one is open.
    unit: Mutex<Option<OpenUnit>>,
}

impl ApplicationLogsService {
    const PREFIX_BLOCK: u8 = 0x40;
    const PREFIX_TX: u8 = 0x41;

    /// Creates a new ApplicationLogs service.
    pub fn new(settings: ApplicationLogsSettings, store: Arc<dyn Store>) -> Self {
        Self {
            settings,
            store,
            snapshot: Mutex::new(None),
            disabled: AtomicBool::new(false),
            accepted: Mutex::new(AcceptedLogs::default()),
            unit: Mutex::new(None),
        }
    }

    /// Returns the settings in use.
    pub fn settings(&self) -> &ApplicationLogsSettings {
        &self.settings
    }

    /// Returns the stored block log JSON, if present.
    pub fn get_block_log(&self, hash: &UInt256) -> Option<Value> {
        self.read_log(Self::PREFIX_BLOCK, hash)
    }

    /// Returns the stored transaction log JSON, if present.
    pub fn get_transaction_log(&self, hash: &UInt256) -> Option<Value> {
        self.read_log(Self::PREFIX_TX, hash)
    }

    fn start_batch(&self) {
        if self.unit.lock().is_some() {
            // A persistence unit stages its own records. Installing a snapshot
            // here would commit them before the chain commit is known.
            return;
        }
        let mut guard = self.snapshot.lock();
        *guard = Some(self.store.snapshot());
    }

    /// Opens an import-scoped persistence unit.
    ///
    /// From this point on, committed callbacks stage their records in memory
    /// instead of committing them to the logs database, so the unit can be
    /// discarded if the matching chain commit fails. Online operation never
    /// opens a unit and keeps its per-block snapshot/commit behaviour.
    pub fn begin_persistence_unit(&self) -> PersistenceUnitOutcome {
        let mut unit = self.unit.lock();
        if unit.is_some() {
            return PersistenceUnitOutcome::AlreadyOpen;
        }
        *unit = Some(OpenUnit::default());
        // A standalone block may have left a snapshot behind; it must not absorb
        // records that belong to this unit.
        self.snapshot.lock().take();
        PersistenceUnitOutcome::Started
    }

    /// Closes the open unit after its physical chain commit succeeded.
    ///
    /// Every staged record becomes accepted-pending: eligible for the next
    /// [`Self::flush_accepted`] but still reversible until that flush runs.
    /// Inside an import session nothing is committed here, so consecutive units
    /// accumulate into one logs write per checkpoint. When the promoted records
    /// push the pending set past the byte cap the commit happens immediately, so
    /// a long checkpoint interval cannot grow the set without bound.
    ///
    /// Returns the commit error of an automatic flush; the records are then left
    /// pending, exactly as a failed explicit flush leaves them.
    ///
    /// An automatic commit is the only case where records reach the logs database
    /// before the next checkpoint's chain flush. That trade-off bounds the memory
    /// a very long checkpoint interval can pin, at the cost of the logs possibly
    /// leading the chain by at most one interval if a crash loses the chain
    /// memtable. The opposite direction — logs behind the chain — is what the
    /// importer detects on resume.
    pub fn finish_persistence_unit(&self) -> Result<PersistenceUnitOutcome, String> {
        let (count, cap_reached) = {
            let mut unit = self.unit.lock();
            let Some(open) = unit.as_mut() else {
                return Ok(PersistenceUnitOutcome::NotOpen);
            };
            let mut records = std::mem::take(&mut open.committed);
            records.append(&mut open.staged);
            let watermark = open.watermark;
            *unit = None;
            let count = records.len();
            let mut accepted = self.accepted.lock();
            accepted.push(records, watermark);
            let cap_reached =
                accepted.session_active && accepted.pending_bytes >= accepted.byte_cap;
            (count, cap_reached)
        };
        if cap_reached {
            // The records are already promoted, so a failure here is reported but
            // must not undo the unit: the next flush retries the same bytes.
            self.flush_accepted()?;
        }
        Ok(PersistenceUnitOutcome::Finished { records: count })
    }

    /// Discards every record staged since [`Self::begin_persistence_unit`].
    ///
    /// Records promoted by an earlier finished unit are preserved, because they
    /// belong to an already committed chain state.
    pub fn abort_persistence_unit(&self) -> PersistenceUnitOutcome {
        let mut unit = self.unit.lock();
        let Some(open) = unit.as_ref() else {
            return PersistenceUnitOutcome::NotOpen;
        };
        let discarded = open.committed.len() + open.staged.len();
        *unit = None;
        self.snapshot.lock().take();
        PersistenceUnitOutcome::Aborted { records: discarded }
    }

    /// Commits all accepted-pending records to the logs database.
    ///
    /// Returns how many records and bytes were written together with the block
    /// watermark those bytes cover. Zero pending records is not an error and
    /// performs no store work. The records are written through a single store
    /// batch, which is the same write path a per-block commit takes, so a caller
    /// that finished one unit per block still gets one logs write per block while
    /// a coalesced session gets one write for the whole checkpoint interval.
    ///
    /// Durability is completed by the caller: an import checkpoint calls
    /// [`Self::try_flush_durable`] right after this commit. The watermark moves
    /// in two stages — this commit parks it as *committed*, and only a successful
    /// durable flush promotes it to *durable*, which is the value
    /// [`Self::watermark`] reports. This method never forces a memtable flush,
    /// because a per-unit call must stay as cheap as the per-block commit it
    /// replaces.
    ///
    /// A failed commit returns the records to the pending set, so a retry writes
    /// the same bytes and the reported pending count keeps describing what is not
    /// yet durable.
    pub fn flush_accepted(&self) -> Result<AcceptedFlush, String> {
        let (records, bytes, watermark) = {
            let mut accepted = self.accepted.lock();
            if accepted.records.is_empty() {
                return Ok(AcceptedFlush::default());
            }
            let records = std::mem::take(&mut accepted.records);
            let bytes = std::mem::take(&mut accepted.pending_bytes);
            let watermark = accepted.pending_watermark.take();
            (records, bytes, watermark)
        };
        let count = u64::try_from(records.len()).unwrap_or(u64::MAX);

        if let Err(err) = self.write_accepted(&records) {
            self.restore_accepted(records, bytes, watermark);
            return Err(err);
        }

        let mut accepted = self.accepted.lock();
        accepted.flushes = accepted.flushes.saturating_add(1);
        accepted.flushed_records = accepted.flushed_records.saturating_add(count);
        accepted.flushed_bytes = accepted.flushed_bytes.saturating_add(bytes);
        if watermark.is_some() {
            accepted.committed_watermark = watermark;
        }
        let committed_watermark = accepted.committed_watermark;
        Ok(AcceptedFlush {
            records: count,
            bytes,
            watermark: committed_watermark,
        })
    }

    /// Writes one batch of accepted records through a single store commit.
    fn write_accepted(&self, records: &[StagedLog]) -> Result<(), String> {
        let mut write = self.store.snapshot();
        let Some(snapshot) = Arc::get_mut(&mut write) else {
            return Err("application logs flush failed: snapshot is still shared".to_string());
        };
        // The records stay owned by the pending set until the commit succeeds, so
        // the bytes are copied into the snapshot instead of being moved.
        for record in records {
            snapshot
                .put(record.key.clone(), record.value.clone())
                .map_err(|err| format!("failed to write staged application log: {err}"))?;
        }
        snapshot
            .try_commit()
            .map_err(|err| format!("application logs flush commit failed: {err}"))?;
        Ok(())
    }

    /// Returns records of a failed commit to the pending set.
    fn restore_accepted(
        &self,
        records: Vec<StagedLog>,
        bytes: u64,
        watermark: Option<(u32, UInt256)>,
    ) {
        let mut accepted = self.accepted.lock();
        accepted.pending_bytes = accepted.pending_bytes.saturating_add(bytes);
        let pending = std::mem::take(&mut accepted.records);
        accepted.records = records;
        accepted.records.extend(pending);
        if accepted.pending_watermark.is_none() {
            accepted.pending_watermark = watermark;
        }
    }

    /// Opens the import-scoped batching session.
    ///
    /// From this point on a finished persistence unit only *promotes* its records
    /// to the accepted-pending set; nothing reaches the logs database until a
    /// [`Self::flush_accepted`] call, which the importer issues at checkpoint
    /// boundaries. Online operation never opens a session, so its per-block
    /// commit stays exactly as it was.
    ///
    /// Opening a session resets the watermark and the flush counters, because
    /// they describe one import run.
    pub fn begin_import_session(&self) -> ImportSessionOutcome {
        let mut accepted = self.accepted.lock();
        if accepted.session_active {
            return ImportSessionOutcome::AlreadyOpen;
        }
        accepted.session_active = true;
        accepted.committed_watermark = None;
        accepted.durable_watermark = None;
        accepted.flushes = 0;
        accepted.flushed_records = 0;
        accepted.flushed_bytes = 0;
        ImportSessionOutcome::Started
    }

    /// Commits the records still pending and leaves batching mode.
    ///
    /// The counters and the watermark stay readable afterwards, so the importer
    /// can report and verify them once the session is closed.
    pub fn end_import_session(&self) -> Result<ImportSessionOutcome, String> {
        if !self.accepted.lock().session_active {
            return Ok(ImportSessionOutcome::NotOpen);
        }
        let flushed = self.flush_accepted()?;
        self.accepted.lock().session_active = false;
        Ok(ImportSessionOutcome::Ended {
            records: flushed.records,
            bytes: flushed.bytes,
        })
    }

    /// Returns whether this service is in import batching mode.
    pub fn import_session_active(&self) -> bool {
        self.accepted.lock().session_active
    }

    /// Sets the byte cap that triggers an automatic commit.
    ///
    /// A cap of zero commits after every finished unit, which reproduces the
    /// per-block write pattern of online operation.
    pub fn set_accepted_byte_cap(&self, bytes: u64) {
        self.accepted.lock().byte_cap = bytes;
    }

    /// Returns the highest block whose records are durable in the logs database.
    ///
    /// `None` means nothing was committed in this import session. Import resume
    /// validation compares this against the chain height: a logs watermark
    /// behind the chain is what a crash between the two store flushes leaves.
    pub fn watermark(&self) -> Option<(u32, UInt256)> {
        self.accepted.lock().durable_watermark
    }

    /// Returns the logs persistence telemetry of this import session.
    pub fn flush_telemetry(&self) -> LogsFlushTelemetry {
        let accepted = self.accepted.lock();
        LogsFlushTelemetry {
            flushes: accepted.flushes,
            records: accepted.flushed_records,
            bytes: accepted.flushed_bytes,
            pending_records: u64::try_from(accepted.records.len()).unwrap_or(u64::MAX),
            watermark: accepted.durable_watermark,
        }
    }

    /// Forces the committed logs to durable storage and advances the watermark
    /// they cover.
    ///
    /// Import uses a RocksDB profile with the write-ahead log disabled, so a
    /// checkpoint must flush this store or a restart drops the logs. The flush is
    /// fallible for exactly that reason: [`Store::flush`] logs a backend error and
    /// returns, which cannot back a success report, so this method surfaces the
    /// error instead.
    ///
    /// The watermark is promoted only after the backend confirms the flush. A
    /// failure therefore leaves the logs certified only up to their last durable
    /// commit, and the caller fails its checkpoint instead of reporting one.
    pub fn try_flush_durable(&self) -> Result<(), String> {
        self.store
            .try_flush_durable()
            .map_err(|err| format!("application logs durable flush failed: {err}"))?;
        let mut accepted = self.accepted.lock();
        if accepted.committed_watermark.is_some() {
            accepted.durable_watermark = accepted.committed_watermark;
        }
        Ok(())
    }

    /// Flushes the logs to durable storage, best effort.
    ///
    /// Startup and the offline rewrite paths only want the bytes written before
    /// the process moves on, so they log a backend failure instead of aborting a
    /// node. The import checkpoint must fail closed and therefore calls
    /// [`Self::try_flush_durable`] directly.
    pub fn flush(&self) {
        if let Err(err) = self.try_flush_durable() {
            warn!(target: "neo", error = %err, "best-effort application logs flush failed");
        }
    }

    /// Rewrites OnPersist and PostPersist `vmstate` values that were stored as
    /// the engine's initial `BREAK`. Those triggers run outside the VM loop and
    /// C# records a successful one as `HALT`. Committed blocks only reach this
    /// store when native persist succeeded, so `BREAK` there is that initial
    /// state, not a breakpoint.
    pub fn rewrite_native_persist_break_to_halt(&self) -> Result<u64, String> {
        use neo_storage::persistence::seek_direction::SeekDirection;

        let prefix = vec![Self::PREFIX_BLOCK];
        let read = self.store.snapshot();
        let matched: Vec<(Vec<u8>, Vec<u8>)> = read
            .find(Some(&prefix), SeekDirection::Forward)
            .take_while(|(key, _)| key.first().copied() == Some(Self::PREFIX_BLOCK))
            .collect();
        drop(read);

        let mut changed = 0u64;
        let mut write = self.store.snapshot();
        let Some(snapshot) = Arc::get_mut(&mut write) else {
            return Err("application logs rewrite failed: snapshot is still shared".to_string());
        };
        for (key, raw) in matched {
            let Ok(mut value) = serde_json::from_slice::<Value>(&raw) else {
                continue;
            };
            if !promote_native_persist_vmstate(&mut value) {
                continue;
            }
            let bytes = serde_json::to_vec(&value)
                .map_err(|err| format!("failed to serialize rewritten application log: {err}"))?;
            snapshot
                .put(key, bytes)
                .map_err(|err| format!("failed to rewrite application log: {err}"))?;
            changed += 1;
        }
        snapshot
            .try_commit()
            .map_err(|err| format!("application logs rewrite commit failed: {err}"))?;
        Ok(changed)
    }

    /// Rewrites stored fault exception text so it matches the C# application log.
    ///
    /// Gas exhaustion becomes `Insufficient GAS.`, a trailing VM diagnostic
    /// suffix is removed, and a missing-method fault drops the Rust
    /// `System.Contract.Call` wrapper. See `csharp_fault_exception_message`.
    pub fn rewrite_fault_exception_messages(&self) -> Result<u64, String> {
        use neo_storage::persistence::seek_direction::SeekDirection;

        let prefixes = [Self::PREFIX_BLOCK, Self::PREFIX_TX];
        let mut matched = Vec::new();
        {
            let read = self.store.snapshot();
            for prefix in prefixes {
                let prefix_key = vec![prefix];
                matched.extend(
                    read.find(Some(&prefix_key), SeekDirection::Forward)
                        .take_while(|(key, _)| key.first().copied() == Some(prefix)),
                );
            }
        }

        let mut changed = 0u64;
        let mut write = self.store.snapshot();
        let Some(snapshot) = Arc::get_mut(&mut write) else {
            return Err("application logs rewrite failed: snapshot is still shared".to_string());
        };
        for (key, raw) in matched {
            let Ok(mut value) = serde_json::from_slice::<Value>(&raw) else {
                continue;
            };
            if !normalize_stored_fault_exceptions(&mut value) {
                continue;
            }
            let bytes = serde_json::to_vec(&value)
                .map_err(|err| format!("failed to serialize rewritten application log: {err}"))?;
            snapshot
                .put(key, bytes)
                .map_err(|err| format!("failed to rewrite application log: {err}"))?;
            changed += 1;
        }
        snapshot
            .try_commit()
            .map_err(|err| format!("application logs rewrite commit failed: {err}"))?;
        Ok(changed)
    }

    fn commit_batch(&self) -> Result<(), String> {
        {
            let mut unit = self.unit.lock();
            if let Some(open) = unit.as_mut() {
                // Inside a unit the committed callback only promotes this block's
                // records. `finish` moves them out and `abort` drops them, so no
                // records reach the database before the chain commit is known.
                let staged = std::mem::take(&mut open.staged);
                open.committed.extend(staged);
                return Ok(());
            }
        }
        let mut guard = self.snapshot.lock();
        let Some(snapshot_arc) = guard.as_mut() else {
            return Ok(());
        };
        let Some(snapshot) = Arc::get_mut(snapshot_arc) else {
            return Err("application logs commit failed: snapshot is still shared".to_string());
        };
        snapshot
            .try_commit()
            .map_err(|err| format!("application logs commit failed: {err}"))
    }

    fn handle_panic(&self, payload: Box<dyn Any + Send>, phase: &'static str) {
        error!(
            target: "neo::application_logs",
            phase,
            error = panic_message(payload.as_ref(), "unknown panic payload"),
            "application logs handler panicked"
        );
        self.settings
            .exception_policy
            .apply(|| self.disabled.store(true, Ordering::SeqCst));
    }

    fn handle_error(&self, err: &str, phase: &'static str) {
        error!(
            target: "neo::application_logs",
            phase,
            error = err,
            "application logs handler failed"
        );
        self.settings
            .exception_policy
            .apply(|| self.disabled.store(true, Ordering::SeqCst));
    }

    /// Records which block the open unit is about to write records for.
    ///
    /// The boundary is what makes a commit report a durable height. Outside a
    /// unit (online mode) nothing is tracked, because that path commits inside
    /// the committed callback and has no interval to describe.
    fn note_block_boundary(&self, height: u32, hash: UInt256) {
        let mut unit = self.unit.lock();
        if let Some(open) = unit.as_mut() {
            open.watermark = Some((height, hash));
        }
    }

    fn write_log(&self, prefix: u8, hash: &UInt256, value: Value) -> Result<(), String> {
        let mut key = Vec::with_capacity(1 + 32);
        key.push(prefix);
        key.extend_from_slice(&hash.to_bytes());
        let bytes = serde_json::to_vec(&value)
            .map_err(|err| format!("failed to serialize application log: {err}"))?;

        {
            let mut unit = self.unit.lock();
            if let Some(open) = unit.as_mut() {
                open.staged.push(StagedLog { key, value: bytes });
                return Ok(());
            }
        }

        let mut guard = self.snapshot.lock();
        let Some(snapshot_arc) = guard.as_mut() else {
            return Ok(());
        };
        let Some(snapshot) = Arc::get_mut(snapshot_arc) else {
            return Err("application logs write failed: snapshot is still shared".to_string());
        };
        snapshot
            .put(key, bytes)
            .map_err(|err| format!("failed to write application log to storage: {err}"))?;
        Ok(())
    }

    fn read_log(&self, prefix: u8, hash: &UInt256) -> Option<Value> {
        let mut key = Vec::with_capacity(1 + 32);
        key.push(prefix);
        key.extend_from_slice(&hash.to_bytes());
        let snapshot = self.store.snapshot();
        let raw = snapshot.try_get(&key)?;
        serde_json::from_slice(&raw).ok()
    }

    fn block_log_json(&self, block_hash: &UInt256, executions: &[ApplicationExecuted]) -> Value {
        let block_executions = executions
            .iter()
            .filter(|exec| exec.transaction.is_none())
            .map(|exec| self.execution_to_json(exec, false))
            .collect::<Vec<_>>();
        let mut obj = Map::new();
        obj.insert(
            "blockhash".to_string(),
            Value::String(block_hash.to_string()),
        );
        obj.insert("executions".to_string(), Value::Array(block_executions));
        Value::Object(obj)
    }

    fn transaction_log_json(&self, tx_hash: &UInt256, exec: &ApplicationExecuted) -> Value {
        let mut obj = Map::new();
        obj.insert("txid".to_string(), Value::String(tx_hash.to_string()));
        obj.insert(
            "executions".to_string(),
            Value::Array(vec![self.execution_to_json(exec, true)]),
        );
        Value::Object(obj)
    }

    fn execution_to_json(&self, exec: &ApplicationExecuted, include_exception: bool) -> Value {
        let mut trigger = Map::new();
        trigger.insert(
            "trigger".to_string(),
            Value::String(trigger_to_string(exec.trigger).to_string()),
        );
        trigger.insert(
            "vmstate".to_string(),
            Value::String(vm_state_to_string(exec.vm_state).to_string()),
        );
        trigger.insert(
            "gasconsumed".to_string(),
            Value::String(exec.gas_consumed.to_string()),
        );

        let mut exception = include_exception.then(|| exec.exception.clone()).flatten();
        let stack_items: &[StackItem] = &exec.stack;
        match stack_items_rpc_json_per_item(stack_items, self.settings.max_stack_size) {
            Ok(stack) => {
                trigger.insert("stack".to_string(), Value::Array(stack));
            }
            Err(err) => {
                exception = Some(err.to_string());
            }
        }
        if let Some(message) = exception.as_mut() {
            let normalized = csharp_fault_exception_message(message);
            if normalized != *message {
                *message = normalized;
            }
        }

        if include_exception || exception.is_some() {
            trigger.insert(
                "exception".to_string(),
                exception.map_or(Value::Null, Value::String),
            );
        }

        let notifications = exec
            .notifications
            .iter()
            .map(notification_to_json)
            .collect::<Vec<_>>();
        trigger.insert("notifications".to_string(), Value::Array(notifications));

        if self.settings.debug {
            let logs = exec
                .logs
                .iter()
                .map(|log| {
                    let mut obj = Map::new();
                    obj.insert(
                        "contract".to_string(),
                        Value::String(log.script_hash.to_string()),
                    );
                    obj.insert("message".to_string(), Value::String(log.message.clone()));
                    Value::Object(obj)
                })
                .collect();
            trigger.insert("logs".to_string(), Value::Array(logs));
        }

        Value::Object(trigger)
    }
}

impl CommittingHandler for ApplicationLogsService {
    fn run_during_fast_sync(&self) -> bool {
        // `.acc` import enables fast sync. Application logs are the behaviour
        // oracle for that import, so this handler must still run.
        true
    }

    fn blockchain_committing_handler(
        &self,
        system: &dyn Any,
        block: &ExecutionBlock,
        _snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
    ) {
        if self.disabled.load(Ordering::Relaxed) {
            return;
        }
        let Some(system) = system.downcast_ref::<NeoSystem>() else {
            return;
        };
        if system.settings().network != self.settings.network {
            return;
        }
        let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
            self.start_batch();

            let block_hash = block.hash();
            // A unit can span several blocks (a coalesced run of empty blocks),
            // and the callbacks arrive in ascending height, so the newest note
            // wins and the unit reports the height it actually reaches.
            self.note_block_boundary(block.index(), block_hash);
            let block_log = self.block_log_json(&block_hash, application_executed_list);
            self.write_log(Self::PREFIX_BLOCK, &block_hash, block_log)?;

            for exec in application_executed_list {
                let Some(tx) = exec.transaction.as_ref() else {
                    continue;
                };
                let tx_hash = tx.hash();
                let tx_log = self.transaction_log_json(&tx_hash, exec);
                self.write_log(Self::PREFIX_TX, &tx_hash, tx_log)?;
            }
            Ok(())
        }));
        match result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => self.handle_error(&err, "committing"),
            Err(payload) => self.handle_panic(payload, "committing"),
        }
    }
}

impl CommittedHandler for ApplicationLogsService {
    fn blockchain_committed_handler(&self, system: &dyn Any, _block: &ExecutionBlock) {
        if self.disabled.load(Ordering::Relaxed) {
            return;
        }
        let Some(system) = system.downcast_ref::<NeoSystem>() else {
            return;
        };
        if system.settings().network != self.settings.network {
            return;
        }
        let result = panic::catch_unwind(AssertUnwindSafe(|| self.commit_batch()));
        match result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => self.handle_error(&err, "committed"),
            Err(payload) => self.handle_panic(payload, "committed"),
        }
    }
}

fn trigger_to_string(trigger: TriggerType) -> &'static str {
    if trigger == TriggerType::ON_PERSIST {
        "OnPersist"
    } else if trigger == TriggerType::POST_PERSIST {
        "PostPersist"
    } else if trigger == TriggerType::VERIFICATION {
        "Verification"
    } else if trigger == TriggerType::APPLICATION {
        "Application"
    } else if trigger == TriggerType::SYSTEM {
        "System"
    } else if trigger == TriggerType::ALL {
        "All"
    } else {
        "Unknown"
    }
}

/// Application-log exception text aligned with C# `ExecutionLogState`.
///
/// C# stores `Exception.InnerException.Message`, or `Exception.Message` when
/// there is no inner exception. `ApplicationEngine.AddFee` throws
/// `InvalidOperationException("Insufficient GAS.")`. Rust records the same
/// condition with its own wording, and `ExecutionEngine::on_fault` appends
/// ` [ip=N opcode=NAME eval_depth=N]`. That suffix is diagnostic only.
/// C# `OpCode.ASSERT` uses `ASSERT is executed with false result.` Rust's
/// `AssertFailed` display omits the period. C# `CallContract` reports a missing
/// method as `Method "name" with N parameter(s) doesn't exist in the contract 0xhash.`
/// Rust's VM display wraps that fault in `Interop service error` and
/// `Invalid operation`, and `UInt160`'s debug text adds a type name.
pub(crate) fn csharp_fault_exception_message(message: &str) -> String {
    let stripped = strip_vm_diagnostic_suffix(message);
    if is_engine_gas_fault(stripped) {
        "Insufficient GAS.".to_string()
    } else if stripped == "ASSERT is executed with false result" {
        "ASSERT is executed with false result.".to_string()
    } else if let Some(missing_method) = csharp_missing_contract_method(stripped) {
        missing_method
    } else {
        stripped.to_string()
    }
}

fn csharp_missing_contract_method(message: &str) -> Option<String> {
    let message = unwrap_interop_service_error(message);
    let message = message
        .strip_prefix("Invalid operation: ")
        .unwrap_or(message);
    let rest = message.strip_prefix("Method ")?;
    let mut chars = rest.chars();
    let quote = chars.next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let rest = chars.as_str();
    let end = rest.find(quote)?;
    let name = &rest[..end];
    if name.is_empty() {
        return None;
    }
    let rest = rest[end + quote.len_utf8()..].strip_prefix(" with ")?;
    let count_len = rest.bytes().take_while(u8::is_ascii_digit).count();
    if count_len == 0 {
        return None;
    }
    let count = &rest[..count_len];
    let rest = rest[count_len..].strip_prefix(" parameter(s) doesn't exist in the contract ")?;
    let (hash, tail) = contract_hash_text(rest)?;
    if tail != "." {
        return None;
    }
    Some(format!(
        "Method \"{name}\" with {count} parameter(s) doesn't exist in the contract {hash}."
    ))
}

fn unwrap_interop_service_error(message: &str) -> &str {
    const PREFIX: &str = "Interop service error: service ";
    const MARKER: &str = ", error: ";
    let Some(rest) = message.strip_prefix(PREFIX) else {
        return message;
    };
    let Some(index) = rest.find(MARKER) else {
        return message;
    };
    if rest[..index].is_empty() {
        return message;
    }
    let inner = &rest[index + MARKER.len()..];
    if inner.is_empty() { message } else { inner }
}

fn contract_hash_text(text: &str) -> Option<(&str, &str)> {
    if let Some(inner) = text.strip_prefix("UInt160(") {
        let end = inner.find(')')?;
        let hash = &inner[..end];
        if !is_script_hash_text(hash) {
            return None;
        }
        return Some((hash, &inner[end + 1..]));
    }
    const HASH_LEN: usize = 42;
    if text.len() < HASH_LEN || !is_script_hash_text(&text[..HASH_LEN]) {
        return None;
    }
    Some((&text[..HASH_LEN], &text[HASH_LEN..]))
}

fn is_script_hash_text(text: &str) -> bool {
    let Some(hex) = text.strip_prefix("0x") else {
        return false;
    };
    hex.len() == 40 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn strip_vm_diagnostic_suffix(message: &str) -> &str {
    let Some(start) = message.rfind(" [ip=") else {
        return message;
    };
    let suffix = &message[start..];
    if suffix_is_vm_diagnostic(suffix) {
        &message[..start]
    } else {
        message
    }
}

fn suffix_is_vm_diagnostic(suffix: &str) -> bool {
    let Some(rest) = suffix.strip_prefix(" [ip=") else {
        return false;
    };
    let Some(rest) = rest.strip_suffix(']') else {
        return false;
    };
    let mut parts = rest.split(' ');
    let Some(ip) = parts.next() else {
        return false;
    };
    if ip.is_empty() || !ip.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    match (parts.next(), parts.next(), parts.next()) {
        (None, None, None) => true,
        (Some(opcode), Some(depth), None) => {
            let Some(opcode_name) = opcode.strip_prefix("opcode=") else {
                return false;
            };
            let Some(depth) = depth.strip_prefix("eval_depth=") else {
                return false;
            };
            !opcode_name.is_empty()
                && !depth.is_empty()
                && depth.bytes().all(|byte| byte.is_ascii_digit())
        }
        _ => false,
    }
}

fn is_engine_gas_fault(message: &str) -> bool {
    matches!(
        message,
        "Gas limit exceeded" | "Out of gas" | "VM exceeded gas limit during execution"
    ) || is_counted_gas_fault(message, "Gas exhausted: used ", ", limit ")
        || is_counted_gas_fault(message, "Insufficient gas: required ", ", available ")
}

fn is_counted_gas_fault(message: &str, prefix: &str, separator: &str) -> bool {
    let Some(rest) = message.strip_prefix(prefix) else {
        return false;
    };
    let Some((left, right)) = rest.split_once(separator) else {
        return false;
    };
    !left.is_empty()
        && left.bytes().all(|byte| byte.is_ascii_digit())
        && !right.is_empty()
        && right.bytes().all(|byte| byte.is_ascii_digit())
}

fn normalize_stored_fault_exceptions(value: &mut Value) -> bool {
    let Some(executions) = value.get_mut("executions").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for exec in executions {
        let Some(obj) = exec.as_object_mut() else {
            continue;
        };
        let Some(message) = obj.get("exception").and_then(Value::as_str) else {
            continue;
        };
        let normalized = csharp_fault_exception_message(message);
        if normalized != message {
            obj.insert("exception".to_string(), Value::String(normalized));
            changed = true;
        }
    }
    changed
}

fn promote_native_persist_vmstate(value: &mut Value) -> bool {
    let Some(executions) = value.get_mut("executions").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for exec in executions {
        let Some(obj) = exec.as_object_mut() else {
            continue;
        };
        let trigger = obj.get("trigger").and_then(Value::as_str);
        let vmstate = obj.get("vmstate").and_then(Value::as_str);
        if matches!(trigger, Some("OnPersist" | "PostPersist")) && vmstate == Some("BREAK") {
            obj.insert("vmstate".to_string(), Value::String("HALT".to_string()));
            changed = true;
        }
    }
    changed
}

fn vm_state_to_string(state: VMState) -> &'static str {
    match state {
        VMState::NONE => "NONE",
        VMState::HALT => "HALT",
        VMState::FAULT => "FAULT",
        VMState::BREAK => "BREAK",
    }
}

fn notification_to_json(event: &NotifyEventArgs) -> Value {
    let mut notification = Map::new();
    notification.insert(
        "contract".to_string(),
        Value::String(event.script_hash.to_string()),
    );
    notification.insert(
        "eventname".to_string(),
        Value::String(event.event_name.clone()),
    );

    let state_values = event
        .state
        .iter()
        .map(|item| stack_item_rpc_json(item, None))
        .collect::<Result<Vec<_>, _>>();

    let state = match state_values {
        Ok(values) => {
            let mut state_obj = Map::new();
            state_obj.insert("type".to_string(), Value::String("Array".to_string()));
            state_obj.insert("value".to_string(), Value::Array(values));
            Value::Object(state_obj)
        }
        Err(_) => Value::String("error: recursive reference".to_string()),
    };
    notification.insert("state".to_string(), state);

    Value::Object(notification)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::UnhandledExceptionPolicy;
    use neo_storage::persistence::{
        SeekDirection,
        read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric},
        storage::StorageError,
        store::OnNewSnapshotDelegate,
        write_store::WriteStore,
    };
    use neo_storage::{StorageItem, StorageKey};

    #[derive(Clone)]
    struct FailingStore;

    impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for FailingStore {
        fn try_get(&self, _key: &Vec<u8>) -> Option<Vec<u8>> {
            None
        }

        fn find(
            &self,
            _key_prefix: Option<&Vec<u8>>,
            _direction: SeekDirection,
        ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
            Box::new(std::iter::empty())
        }
    }

    impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for FailingStore {
        fn try_get(&self, _key: &StorageKey) -> Option<StorageItem> {
            None
        }

        fn find(
            &self,
            _key_prefix: Option<&StorageKey>,
            _direction: SeekDirection,
        ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
            Box::new(std::iter::empty())
        }
    }

    impl ReadOnlyStore for FailingStore {}

    impl WriteStore<Vec<u8>, Vec<u8>> for FailingStore {
        fn delete(&mut self, _key: Vec<u8>) -> Result<(), StorageError> {
            Ok(())
        }

        fn put(&mut self, _key: Vec<u8>, _value: Vec<u8>) -> Result<(), StorageError> {
            Ok(())
        }
    }

    impl Store for FailingStore {
        fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
            Arc::new(FailingSnapshot {
                store: Arc::new(self.clone()),
            })
        }

        fn on_new_snapshot(&self, _handler: OnNewSnapshotDelegate) {}

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    struct FailingSnapshot {
        store: Arc<dyn Store>,
    }

    impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for FailingSnapshot {
        fn try_get(&self, _key: &Vec<u8>) -> Option<Vec<u8>> {
            None
        }

        fn find(
            &self,
            _key_prefix: Option<&Vec<u8>>,
            _direction: SeekDirection,
        ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
            Box::new(std::iter::empty())
        }
    }

    impl WriteStore<Vec<u8>, Vec<u8>> for FailingSnapshot {
        fn delete(&mut self, _key: Vec<u8>) -> Result<(), StorageError> {
            Ok(())
        }

        fn put(&mut self, _key: Vec<u8>, _value: Vec<u8>) -> Result<(), StorageError> {
            Err(StorageError::invalid_operation(
                "injected application logs write failure",
            ))
        }
    }

    impl StoreSnapshot for FailingSnapshot {
        fn store(&self) -> Arc<dyn Store> {
            Arc::clone(&self.store)
        }

        fn try_commit(&mut self) -> neo_storage::persistence::store_snapshot::SnapshotCommitResult {
            Err(StorageError::CommitFailed(
                "injected application logs commit failure".to_string(),
            ))
        }
    }

    fn settings(exception_policy: UnhandledExceptionPolicy) -> ApplicationLogsSettings {
        ApplicationLogsSettings {
            exception_policy,
            ..ApplicationLogsSettings::default()
        }
    }

    fn memory_service() -> (
        ApplicationLogsService,
        Arc<neo_storage::persistence::providers::MemoryStore>,
    ) {
        let store = Arc::new(neo_storage::persistence::providers::MemoryStore::new());
        let service = ApplicationLogsService::new(
            settings(UnhandledExceptionPolicy::Ignore),
            Arc::clone(&store) as Arc<dyn Store>,
        );
        (service, store)
    }

    /// A logs store that supports the fallible durable flush interface.
    ///
    /// `MemoryStore` inherits the trait default that rejects explicit durable
    /// flushes, so a checkpoint cannot run against it. This wrapper keeps the
    /// memory storage, adds the flush gate, records how many records the store
    /// already held at each flush, and can be armed to fail.
    struct DurableMemoryStore {
        inner: neo_storage::persistence::providers::MemoryStore,
        flush_fails: AtomicBool,
        flushes: Arc<Mutex<Vec<usize>>>,
    }

    impl DurableMemoryStore {
        fn new(flushes: Arc<Mutex<Vec<usize>>>) -> Self {
            Self {
                inner: neo_storage::persistence::providers::MemoryStore::new(),
                flush_fails: AtomicBool::new(false),
                flushes,
            }
        }

        fn fail_flushes(&self) {
            self.flush_fails.store(true, Ordering::SeqCst);
        }
    }

    impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for DurableMemoryStore {
        fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
            self.inner.try_get(key)
        }

        fn find(
            &self,
            key_prefix: Option<&Vec<u8>>,
            direction: SeekDirection,
        ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
            self.inner.find(key_prefix, direction)
        }
    }

    impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for DurableMemoryStore {
        fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
            self.inner.try_get(key)
        }

        fn find(
            &self,
            key_prefix: Option<&StorageKey>,
            direction: SeekDirection,
        ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
            self.inner.find(key_prefix, direction)
        }
    }

    impl ReadOnlyStore for DurableMemoryStore {}

    impl WriteStore<Vec<u8>, Vec<u8>> for DurableMemoryStore {
        fn delete(&mut self, key: Vec<u8>) -> Result<(), StorageError> {
            self.inner.delete(key)
        }

        fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<(), StorageError> {
            self.inner.put(key, value)
        }
    }

    impl Store for DurableMemoryStore {
        fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
            self.inner.snapshot()
        }

        fn on_new_snapshot(&self, handler: OnNewSnapshotDelegate) {
            self.inner.on_new_snapshot(handler);
        }

        fn try_flush_durable(&self) -> neo_storage::StorageResult<()> {
            let stored_records = self
                .inner
                .find(None::<&Vec<u8>>, SeekDirection::Forward)
                .count();
            self.flushes.lock().push(stored_records);
            if self.flush_fails.load(Ordering::SeqCst) {
                return Err(StorageError::CommitFailed(
                    "injected application logs flush failure".to_string(),
                ));
            }
            Ok(())
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// A logs service over a store that supports the fallible durable flush.
    fn durable_service() -> (
        ApplicationLogsService,
        Arc<DurableMemoryStore>,
        Arc<Mutex<Vec<usize>>>,
    ) {
        let flushes = Arc::new(Mutex::new(Vec::new()));
        let store = Arc::new(DurableMemoryStore::new(Arc::clone(&flushes)));
        let service = ApplicationLogsService::new(
            settings(UnhandledExceptionPolicy::Ignore),
            Arc::clone(&store) as Arc<dyn Store>,
        );
        (service, store, flushes)
    }

    fn block_log_hash(byte: u8) -> UInt256 {
        UInt256::from_bytes(&[byte; 32]).expect("32-byte hash")
    }

    fn stored_block_log(
        store: &neo_storage::persistence::providers::MemoryStore,
        hash: &UInt256,
    ) -> Option<Value> {
        let mut key = vec![ApplicationLogsService::PREFIX_BLOCK];
        key.extend_from_slice(&hash.to_bytes());
        let raw = store.try_get(&key)?;
        serde_json::from_slice(&raw).ok()
    }

    /// Writes one block's records the way the committing callback does,
    /// including the per-block snapshot request that must be a no-op inside a
    /// unit and the block boundary that makes a commit report a height.
    fn commit_one_inside_unit(service: &ApplicationLogsService, hash: &UInt256, height: u32) {
        service.note_block_boundary(height, *hash);
        service.start_batch();
        service
            .write_log(
                ApplicationLogsService::PREFIX_BLOCK,
                hash,
                serde_json::json!({ "blockhash": hash.to_string() }),
            )
            .expect("stage application log");
        service
            .commit_batch()
            .expect("promote staged application log inside the unit");
    }

    /// Persists one block inside its own unit, exactly as the importer does.
    fn finish_one_unit(service: &ApplicationLogsService, hash: &UInt256, height: u32) {
        assert_eq!(
            service.begin_persistence_unit(),
            PersistenceUnitOutcome::Started
        );
        commit_one_inside_unit(service, hash, height);
        assert_eq!(
            service.finish_persistence_unit().expect("finish unit"),
            PersistenceUnitOutcome::Finished { records: 1 }
        );
    }

    #[test]
    fn persistence_unit_abort_discards_records_staged_since_begin() {
        let (service, store) = memory_service();
        let hash = block_log_hash(0x11);

        assert_eq!(
            service.begin_persistence_unit(),
            PersistenceUnitOutcome::Started
        );
        commit_one_inside_unit(&service, &hash, 1);
        assert_eq!(
            service.abort_persistence_unit(),
            PersistenceUnitOutcome::Aborted { records: 1 }
        );

        assert_eq!(service.flush_accepted().expect("flush").records, 0);
        assert!(
            stored_block_log(&store, &hash).is_none(),
            "an aborted unit must not leave any application log behind"
        );
    }

    #[test]
    fn persistence_unit_finish_preserves_records_until_flush() {
        let (service, store) = memory_service();
        let hash = block_log_hash(0x22);

        assert_eq!(
            service.begin_persistence_unit(),
            PersistenceUnitOutcome::Started
        );
        commit_one_inside_unit(&service, &hash, 1);
        assert!(
            stored_block_log(&store, &hash).is_none(),
            "a record staged inside a unit must not reach the store before flush"
        );

        assert_eq!(
            service.finish_persistence_unit().expect("finish unit"),
            PersistenceUnitOutcome::Finished { records: 1 }
        );
        assert!(
            stored_block_log(&store, &hash).is_none(),
            "finishing a unit only promotes records, it does not commit them"
        );

        assert_eq!(service.flush_accepted().expect("flush").records, 1);
        let stored = stored_block_log(&store, &hash).expect("committed block log");
        assert_eq!(
            stored["blockhash"].as_str(),
            Some(hash.to_string().as_str())
        );
        assert_eq!(
            service.flush_accepted().expect("second flush").records,
            0,
            "a flush must not resend records it already committed"
        );
    }

    #[test]
    fn persistence_unit_abort_keeps_records_of_an_earlier_finished_unit() {
        let (service, store) = memory_service();
        let kept = block_log_hash(0x33);
        let dropped = block_log_hash(0x44);

        finish_one_unit(&service, &kept, 1);

        service.begin_persistence_unit();
        commit_one_inside_unit(&service, &dropped, 2);
        assert_eq!(
            service.abort_persistence_unit(),
            PersistenceUnitOutcome::Aborted { records: 1 }
        );

        assert_eq!(service.flush_accepted().expect("flush").records, 1);
        assert!(stored_block_log(&store, &kept).is_some());
        assert!(stored_block_log(&store, &dropped).is_none());
    }

    #[test]
    fn persistence_unit_lifecycle_survives_misuse() {
        let (service, _store) = memory_service();
        let hash = block_log_hash(0x55);

        // finish/abort without an open unit are no-ops, not panics.
        assert_eq!(
            service.finish_persistence_unit().expect("finish unit"),
            PersistenceUnitOutcome::NotOpen
        );
        assert_eq!(
            service.abort_persistence_unit(),
            PersistenceUnitOutcome::NotOpen
        );

        // A second begin keeps the unit that is already open.
        assert_eq!(
            service.begin_persistence_unit(),
            PersistenceUnitOutcome::Started
        );
        commit_one_inside_unit(&service, &hash, 1);
        assert_eq!(
            service.begin_persistence_unit(),
            PersistenceUnitOutcome::AlreadyOpen
        );
        assert_eq!(
            service.finish_persistence_unit().expect("finish unit"),
            PersistenceUnitOutcome::Finished { records: 1 }
        );
        assert_eq!(service.flush_accepted().expect("flush").records, 1);

        // A double finish cannot re-promote anything.
        assert_eq!(
            service.finish_persistence_unit().expect("finish unit"),
            PersistenceUnitOutcome::NotOpen
        );
        assert_eq!(service.flush_accepted().expect("flush").records, 0);
    }

    #[test]
    fn online_mode_keeps_per_block_snapshot_commit() {
        let (service, store) = memory_service();
        let hash = block_log_hash(0x66);

        // No unit is opened in online mode: the committing callback installs a
        // snapshot and the committed callback commits it, exactly as before.
        service.note_block_boundary(1, hash);
        service.start_batch();
        service
            .write_log(
                ApplicationLogsService::PREFIX_BLOCK,
                &hash,
                serde_json::json!({ "blockhash": hash.to_string() }),
            )
            .expect("write application log");
        assert!(stored_block_log(&store, &hash).is_none());

        service.commit_batch().expect("commit application log");
        assert!(stored_block_log(&store, &hash).is_some());
        assert_eq!(
            service.flush_accepted().expect("flush").records,
            0,
            "online mode never parks records in the accepted set"
        );
        assert!(
            !service.import_session_active(),
            "online mode never opens an import session"
        );
        assert_eq!(
            service.watermark(),
            None,
            "nothing is tracked for the online per-block path"
        );
        assert_eq!(service.flush_telemetry(), LogsFlushTelemetry::default());
    }

    #[test]
    fn import_session_coalesces_many_blocks_into_one_commit() {
        let (service, store) = memory_service();
        assert_eq!(
            service.begin_import_session(),
            ImportSessionOutcome::Started
        );

        let hashes = (1..=10u8).map(block_log_hash).collect::<Vec<_>>();
        for (offset, hash) in hashes.iter().enumerate() {
            let height = u32::try_from(offset).expect("test height") + 1;
            finish_one_unit(&service, hash, height);
        }

        // Ten finished units park ten records and write nothing on their own.
        let pending = service.flush_telemetry();
        assert_eq!(pending.flushes, 0);
        assert_eq!(pending.records, 0);
        assert_eq!(pending.pending_records, 10);
        for hash in &hashes {
            assert!(stored_block_log(&store, hash).is_none());
        }

        let ended = service.end_import_session().expect("end session");
        match ended {
            ImportSessionOutcome::Ended { records, bytes } => {
                assert_eq!(
                    records, 10,
                    "the closing commit writes every pending record"
                );
                assert!(bytes > 0, "the closing commit reports the bytes it wrote");
            }
            other => panic!("unexpected session outcome: {other:?}"),
        }

        // One commit carried all ten records, so the ten blocks cost one write.
        let telemetry = service.flush_telemetry();
        assert_eq!(telemetry.flushes, 1);
        assert_eq!(telemetry.records, 10);
        assert!(telemetry.bytes > 0);
        assert_eq!(telemetry.pending_records, 0);
        assert!(!service.import_session_active());
        for hash in &hashes {
            assert!(stored_block_log(&store, hash).is_some());
        }
    }

    /// Dumps every record of a logs store, in key order.
    fn dump_logs(
        store: &neo_storage::persistence::providers::MemoryStore,
    ) -> Vec<(Vec<u8>, Vec<u8>)> {
        store.find(None, SeekDirection::Forward).collect()
    }

    /// The same ten blocks must store identical JSON whether they are committed
    /// one unit at a time or accumulated into one coalesced commit.
    ///
    /// Coalescing only changes *when* records are written, never what is
    /// written: both runs push the same key/value pairs into the same store.
    #[test]
    fn coalesced_commit_stores_the_same_json_as_per_block_commits() {
        let hashes = (1..=10u8).map(block_log_hash).collect::<Vec<_>>();

        // Per-block commits: a zero byte cap commits after every finished unit.
        let (per_block, per_block_store) = memory_service();
        per_block.set_accepted_byte_cap(0);
        per_block.begin_import_session();
        for (offset, hash) in hashes.iter().enumerate() {
            let height = u32::try_from(offset).expect("test height") + 1;
            finish_one_unit(&per_block, hash, height);
        }
        per_block.end_import_session().expect("end session");

        // Coalesced commits: one unit per block, one commit at the end.
        let (coalesced, coalesced_store) = memory_service();
        coalesced.begin_import_session();
        for (offset, hash) in hashes.iter().enumerate() {
            let height = u32::try_from(offset).expect("test height") + 1;
            finish_one_unit(&coalesced, hash, height);
        }
        coalesced.end_import_session().expect("end session");

        assert_eq!(
            per_block.flush_telemetry().flushes,
            u64::try_from(hashes.len()).expect("record count"),
            "the per-block run must commit once per block"
        );
        assert_eq!(
            coalesced.flush_telemetry().flushes,
            1,
            "the coalesced run must commit once for the whole run"
        );

        let per_block_records = dump_logs(&per_block_store);
        assert_eq!(per_block_records.len(), hashes.len());
        assert_eq!(
            per_block_records,
            dump_logs(&coalesced_store),
            "coalescing must not change the stored bytes"
        );
    }

    /// Serialized size of one block-log record, measured through a session that
    /// commits exactly that record.
    fn single_record_bytes() -> u64 {
        let (service, _store) = memory_service();
        service.begin_import_session();
        finish_one_unit(&service, &block_log_hash(0x01), 1);
        service.end_import_session().expect("end session");
        service.flush_telemetry().bytes
    }

    #[test]
    fn accepted_byte_cap_forces_a_commit_mid_accumulation() {
        let record_bytes = single_record_bytes();
        assert!(record_bytes > 0);

        // A cap only two records fit in: the third promotion crosses it, so the
        // commit lands mid-accumulation instead of at the next checkpoint.
        let (service, store) = memory_service();
        service.set_accepted_byte_cap(record_bytes * 2 + 1);
        assert_eq!(
            service.begin_import_session(),
            ImportSessionOutcome::Started
        );

        let hashes = (1..=5u8).map(block_log_hash).collect::<Vec<_>>();
        for (offset, hash) in hashes.iter().enumerate() {
            let height = u32::try_from(offset).expect("test height") + 1;
            finish_one_unit(&service, hash, height);
        }

        let telemetry = service.flush_telemetry();
        assert_eq!(telemetry.flushes, 1, "the cap must commit on its own");
        assert_eq!(telemetry.records, 3, "the commit carries what crossed it");
        assert_eq!(telemetry.pending_records, 2);
        assert!(stored_block_log(&store, &hashes[0]).is_some());
        assert!(stored_block_log(&store, &hashes[2]).is_some());
        assert!(
            stored_block_log(&store, &hashes[3]).is_none(),
            "records after the cap commit stay pending"
        );

        service.end_import_session().expect("end session");
        let telemetry = service.flush_telemetry();
        assert_eq!(telemetry.flushes, 2);
        assert_eq!(telemetry.records, 5);
        assert_eq!(telemetry.pending_records, 0);

        // A cap of zero reproduces the per-block write pattern.
        let (per_block, _store) = memory_service();
        per_block.set_accepted_byte_cap(0);
        per_block.begin_import_session();
        for (offset, hash) in hashes.iter().take(3).enumerate() {
            let height = u32::try_from(offset).expect("test height") + 1;
            finish_one_unit(&per_block, hash, height);
        }
        assert_eq!(
            per_block.flush_telemetry().flushes,
            3,
            "a zero cap commits after every finished unit"
        );
    }

    #[test]
    fn watermark_tracks_the_last_durably_flushed_height() {
        let (service, _store, _flushes) = durable_service();
        assert_eq!(service.watermark(), None);

        service.begin_import_session();
        assert_eq!(
            service.watermark(),
            None,
            "a new session starts without a watermark"
        );

        let first = block_log_hash(0x71);
        finish_one_unit(&service, &first, 5);
        assert_eq!(
            service.watermark(),
            None,
            "promoting a unit is not durability"
        );

        let flushed = service.flush_accepted().expect("flush");
        assert_eq!(flushed.records, 1);
        assert_eq!(
            flushed.watermark,
            Some((5, first)),
            "the commit reports the block its bytes cover"
        );
        assert_eq!(
            service.watermark(),
            None,
            "a commit alone must not certify durability"
        );

        service.try_flush_durable().expect("durable flush");
        assert_eq!(service.watermark(), Some((5, first)));

        let second = block_log_hash(0x72);
        finish_one_unit(&service, &second, 9);
        service.flush_accepted().expect("flush");
        service.try_flush_durable().expect("durable flush");
        assert_eq!(service.watermark(), Some((9, second)));

        // Ending the session keeps the watermark readable for resume validation.
        service.end_import_session().expect("end session");
        assert_eq!(service.watermark(), Some((9, second)));

        // The next session starts from nothing again.
        service.begin_import_session();
        assert_eq!(service.watermark(), None);
    }

    /// The watermark is the logs database's durable claim, so it may only move
    /// after the backend confirmed the flush.
    #[test]
    fn durable_flush_advances_the_watermark_only_after_the_store_flush() {
        let (service, _store, flushes) = durable_service();
        service.begin_import_session();

        let hash = block_log_hash(0xC1);
        finish_one_unit(&service, &hash, 4);
        let flushed = service.flush_accepted().expect("commit");
        assert_eq!(flushed.records, 1);
        assert_eq!(service.watermark(), None, "the commit is not durability");

        service.try_flush_durable().expect("durable flush");
        assert_eq!(service.watermark(), Some((4, hash)));
        assert_eq!(service.flush_telemetry().watermark, Some((4, hash)));

        // The record was already in the store when the flush ran, so the commit
        // preceded it, and the watermark was still unset right before the call, so
        // its promotion can only have happened inside it.
        let observed = flushes.lock();
        assert_eq!(observed.len(), 1, "one commit must cost one durable flush");
        assert_eq!(observed[0], 1, "the record was committed before the flush");
    }

    #[test]
    fn durable_flush_failure_keeps_the_watermark_unadvanced() {
        let (service, store, flushes) = durable_service();
        service.begin_import_session();

        let hash = block_log_hash(0xD1);
        finish_one_unit(&service, &hash, 6);
        assert_eq!(service.flush_accepted().expect("commit").records, 1);
        assert!(stored_block_log(&store.inner, &hash).is_some());
        assert_eq!(service.watermark(), None);

        store.fail_flushes();
        let error = service
            .try_flush_durable()
            .expect_err("a failing durable flush must surface its error");
        assert!(
            error.contains("injected application logs flush failure"),
            "unexpected error: {error}"
        );
        assert_eq!(
            service.watermark(),
            None,
            "a failed flush must not advance the watermark"
        );
        assert_eq!(
            service.flush_telemetry().watermark,
            None,
            "the telemetry must not report an undurable watermark either"
        );
        assert!(
            stored_block_log(&store.inner, &hash).is_some(),
            "a failed flush must not undo the records the commit wrote"
        );
        assert_eq!(*flushes.lock(), vec![1], "the failing flush still ran");
    }

    #[test]
    fn import_session_lifecycle_survives_misuse() {
        let (service, store) = memory_service();

        assert_eq!(
            service.end_import_session().expect("end session"),
            ImportSessionOutcome::NotOpen,
            "ending without a session must not write anything"
        );
        assert!(!service.import_session_active());

        assert_eq!(
            service.begin_import_session(),
            ImportSessionOutcome::Started
        );
        assert_eq!(
            service.begin_import_session(),
            ImportSessionOutcome::AlreadyOpen,
            "a second begin keeps the open session"
        );

        let hash = block_log_hash(0x91);
        finish_one_unit(&service, &hash, 1);
        match service.end_import_session().expect("end session") {
            ImportSessionOutcome::Ended { records, bytes } => {
                assert_eq!(records, 1);
                assert!(bytes > 0);
            }
            other => panic!("unexpected session outcome: {other:?}"),
        }
        assert!(stored_block_log(&store, &hash).is_some());
        assert_eq!(
            service.end_import_session().expect("second end"),
            ImportSessionOutcome::NotOpen
        );
        assert!(!service.import_session_active());
    }

    #[test]
    fn failed_commit_returns_the_records_to_the_pending_set() {
        let service = ApplicationLogsService::new(
            settings(UnhandledExceptionPolicy::Ignore),
            Arc::new(FailingStore),
        );
        service.begin_import_session();
        finish_one_unit(&service, &block_log_hash(0x81), 1);

        let error = service.flush_accepted().expect_err("write failure");
        assert!(error.contains("injected application logs write failure"));

        let telemetry = service.flush_telemetry();
        assert_eq!(telemetry.flushes, 0);
        assert_eq!(telemetry.records, 0);
        assert_eq!(
            telemetry.pending_records, 1,
            "a failed commit must keep its records pending"
        );
        assert_eq!(service.watermark(), None);
    }

    #[test]
    fn automatic_cap_commit_failure_is_reported_and_keeps_records_pending() {
        let service = ApplicationLogsService::new(
            settings(UnhandledExceptionPolicy::Ignore),
            Arc::new(FailingStore),
        );
        service.set_accepted_byte_cap(0);
        service.begin_import_session();
        service.begin_persistence_unit();
        commit_one_inside_unit(&service, &block_log_hash(0xA1), 1);

        let error = service
            .finish_persistence_unit()
            .expect_err("the automatic cap commit must report its failure");
        assert!(error.contains("injected application logs write failure"));
        assert_eq!(service.flush_telemetry().pending_records, 1);
    }

    #[test]
    fn application_logs_run_during_fast_sync_import() {
        let service = ApplicationLogsService::new(
            settings(UnhandledExceptionPolicy::Ignore),
            Arc::new(FailingStore),
        );
        assert!(CommittingHandler::run_during_fast_sync(&service));
    }

    #[test]
    fn commit_batch_propagates_snapshot_try_commit_failure() {
        let service = ApplicationLogsService::new(
            settings(UnhandledExceptionPolicy::Ignore),
            Arc::new(FailingStore),
        );
        service.start_batch();

        let err = service
            .commit_batch()
            .expect_err("application logs commit should propagate snapshot commit failure");

        assert!(err.contains("injected application logs commit failure"));
    }

    #[test]
    fn write_log_propagates_snapshot_put_failure() {
        let service = ApplicationLogsService::new(
            settings(UnhandledExceptionPolicy::Ignore),
            Arc::new(FailingStore),
        );
        service.start_batch();

        let err = service
            .write_log(
                ApplicationLogsService::PREFIX_BLOCK,
                &UInt256::zero(),
                Value::Null,
            )
            .expect_err("application logs write should propagate snapshot put failure");

        assert!(err.contains("injected application logs write failure"));
    }

    #[test]
    fn commit_error_disables_service_when_policy_stops_plugin() {
        let service = ApplicationLogsService::new(
            settings(UnhandledExceptionPolicy::StopPlugin),
            Arc::new(FailingStore),
        );

        service.handle_error("injected failure", "committed");

        assert!(service.disabled.load(Ordering::SeqCst));
    }

    #[test]
    fn csharp_fault_exception_message_matches_addfee_text() {
        assert_eq!(
            csharp_fault_exception_message(
                "Gas exhausted: used 30, limit 1 [ip=0 opcode=PUSH1 eval_depth=0]"
            ),
            "Insufficient GAS."
        );
        assert_eq!(
            csharp_fault_exception_message("Gas exhausted: used 30, limit 1"),
            "Insufficient GAS."
        );
        assert_eq!(
            csharp_fault_exception_message("Insufficient gas: required 30, available 1 [ip=4]"),
            "Insufficient GAS."
        );
        assert_eq!(
            csharp_fault_exception_message("Gas limit exceeded"),
            "Insufficient GAS."
        );
        assert_eq!(
            csharp_fault_exception_message("Out of gas"),
            "Insufficient GAS."
        );
        assert_eq!(
            csharp_fault_exception_message("VM exceeded gas limit during execution"),
            "Insufficient GAS."
        );
        assert_eq!(
            csharp_fault_exception_message("Insufficient GAS."),
            "Insufficient GAS."
        );
        assert_eq!(
            csharp_fault_exception_message(
                "Called Contract Does Not Exist: 0xabc [ip=12 opcode=SYSCALL eval_depth=1]"
            ),
            "Called Contract Does Not Exist: 0xabc"
        );
        assert_eq!(
            csharp_fault_exception_message("user threw Gas exhausted: used 1, limit 1 extra"),
            "user threw Gas exhausted: used 1, limit 1 extra"
        );
        assert_eq!(
            csharp_fault_exception_message("keep [ip=not-a-diagnostic]"),
            "keep [ip=not-a-diagnostic]"
        );
        assert_eq!(
            csharp_fault_exception_message("ASSERT is executed with false result"),
            "ASSERT is executed with false result."
        );
        assert_eq!(
            csharp_fault_exception_message(
                "ASSERT is executed with false result [ip=8 opcode=ASSERT eval_depth=1]"
            ),
            "ASSERT is executed with false result."
        );
        assert_eq!(
            csharp_fault_exception_message("ASSERT is executed with false result: paused"),
            "ASSERT is executed with false result: paused"
        );
        assert_eq!(
            csharp_fault_exception_message(
                "Interop service error: service System.Contract.Call, error: Invalid operation: Method 'pause' with 0 parameter(s) doesn't exist in the contract UInt160(0x385501cb0288b2953db767b0dc971478539456cb)."
            ),
            "Method \"pause\" with 0 parameter(s) doesn't exist in the contract 0x385501cb0288b2953db767b0dc971478539456cb."
        );
        assert_eq!(
            csharp_fault_exception_message(
                "Interop service error: service System.Contract.Call, error: Invalid operation: Method \"pause\" with 0 parameter(s) doesn't exist in the contract 0x385501cb0288b2953db767b0dc971478539456cb. [ip=20 opcode=SYSCALL eval_depth=2]"
            ),
            "Method \"pause\" with 0 parameter(s) doesn't exist in the contract 0x385501cb0288b2953db767b0dc971478539456cb."
        );
        assert_eq!(
            csharp_fault_exception_message(
                "Method \"pause\" with 0 parameter(s) doesn't exist in the contract 0x385501cb0288b2953db767b0dc971478539456cb."
            ),
            "Method \"pause\" with 0 parameter(s) doesn't exist in the contract 0x385501cb0288b2953db767b0dc971478539456cb."
        );
        assert_eq!(
            csharp_fault_exception_message(
                "Method 'pause' with 0 parameter(s) doesn't exist in the contract UInt160(0x385501cb0288b2953db767b0dc971478539456cb). extra"
            ),
            "Method 'pause' with 0 parameter(s) doesn't exist in the contract UInt160(0x385501cb0288b2953db767b0dc971478539456cb). extra"
        );
        assert_eq!(
            csharp_fault_exception_message(
                "Interop service error: service System.Contract.Call, error: Invalid operation: something else"
            ),
            "Interop service error: service System.Contract.Call, error: Invalid operation: something else"
        );

        let mut value = serde_json::json!({
            "executions": [{
                "exception": "Gas exhausted: used 30, limit 1 [ip=0 opcode=PUSH1 eval_depth=0]"
            }]
        });
        assert!(normalize_stored_fault_exceptions(&mut value));
        assert_eq!(
            value["executions"][0]["exception"].as_str(),
            Some("Insufficient GAS.")
        );
        assert!(!normalize_stored_fault_exceptions(&mut value));
    }
}
