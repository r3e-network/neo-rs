//! ApplicationLogs service for capturing execution logs and serving RPC queries.

use neo_ledger::ApplicationExecuted;
use neo_primitives::TriggerType;
use neo_primitives::UInt256;
use neo_primitives::panic_message;
use neo_protocol::execution::Block as ExecutionBlock;
use neo_rpc_client::serialization::stack_item_json::{
    stack_item_rpc_json, stack_items_rpc_json_per_item,
};
use neo_storage::persistence::{PersistenceDataCache, Store, StoreSnapshot};
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

mod fault_messages;
mod handlers;
mod json;
#[cfg(test)]
mod tests;
mod types;

use fault_messages::{
    csharp_fault_exception_message, normalize_stored_fault_exceptions,
    promote_native_persist_vmstate,
};
use json::{notification_to_json, trigger_to_string, vm_state_to_string};
pub use types::{
    AcceptedFlush, DEFAULT_ACCEPTED_BYTE_CAP, ImportSessionOutcome, LogsFlushTelemetry,
    PersistenceUnitOutcome,
};
use types::{AcceptedLogs, OpenUnit, StagedLog};

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
