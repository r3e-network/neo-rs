//! ApplicationLogs service for capturing execution logs and serving RPC queries.

use neo_ledger::ApplicationExecuted;
use neo_primitives::TriggerType;
use neo_primitives::UInt256;
use neo_primitives::panic_message;
use neo_protocol::execution::Block as ExecutionBlock;
use neo_storage::persistence::{DataCache, Store, StoreSnapshot};
use neo_system::NeoSystem;
use neo_system::i_event_handlers::{CommittedHandler, CommittingHandler};
use neo_vm::NotifyEventArgs;
use neo_vm::StackItem;
use neo_vm::VmState as VMState;
use neo_vm::rpc_json::{stack_item_rpc_json, stack_items_rpc_json_per_item};
use parking_lot::Mutex;
use serde_json::{Map, Value};
use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::error;

use super::ApplicationLogsSettings;

/// ApplicationLogs storage and commit handler.
pub struct ApplicationLogsService {
    settings: ApplicationLogsSettings,
    store: Arc<dyn Store>,
    snapshot: Mutex<Option<Arc<dyn StoreSnapshot>>>,
    disabled: AtomicBool,
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
        let mut guard = self.snapshot.lock();
        *guard = Some(self.store.snapshot());
    }

    /// Flushes committed logs to durable storage.
    ///
    /// Import uses a RocksDB profile with the write-ahead log disabled, so a
    /// checkpoint must flush this store or a restart drops the logs.
    pub fn flush(&self) {
        self.store.flush();
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

    fn write_log(&self, prefix: u8, hash: &UInt256, value: Value) -> Result<(), String> {
        let mut guard = self.snapshot.lock();
        let Some(snapshot_arc) = guard.as_mut() else {
            return Ok(());
        };
        let Some(snapshot) = Arc::get_mut(snapshot_arc) else {
            return Err("application logs write failed: snapshot is still shared".to_string());
        };
        let mut key = Vec::with_capacity(1 + 32);
        key.push(prefix);
        key.extend_from_slice(&hash.to_bytes());
        let bytes = serde_json::to_vec(&value)
            .map_err(|err| format!("failed to serialize application log: {err}"))?;
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
