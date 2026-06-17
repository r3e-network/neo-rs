//! ApplicationLogs service for capturing execution logs and serving RPC queries.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_error::{CoreError, CoreResult};
use neo_execution::NotifyEventArgs;
use neo_payloads::ApplicationExecuted;
use neo_payloads::Block;
use neo_payloads::{CommittedHandler, CommittingHandler};
use neo_primitives::TriggerType;
use neo_primitives::UInt256;
use neo_primitives::panic_message;
use neo_storage::persistence::{DataCache, Store, StoreSnapshot};
use neo_system::Node;
use neo_vm::rpc_json::StackItemRpcJson;
use neo_vm_rs::{StackValue, VmState as VMState};
use num_bigint::BigInt;
use parking_lot::Mutex;
use serde_json::{Map, Number as JsonNumber, Value};
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

    fn commit_batch(&self) -> CoreResult<()> {
        let mut guard = self.snapshot.lock();
        let Some(snapshot_arc) = guard.as_mut() else {
            return Ok(());
        };
        let Some(snapshot) = Arc::get_mut(snapshot_arc) else {
            return Err(CoreError::other(
                "application logs commit failed: snapshot is still shared",
            ));
        };
        snapshot
            .try_commit()
            .map_err(|err| CoreError::other(format!("application logs commit failed: {err}")))
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

    fn write_log(&self, prefix: u8, hash: &UInt256, value: Value) -> CoreResult<()> {
        let mut guard = self.snapshot.lock();
        let Some(snapshot_arc) = guard.as_mut() else {
            return Ok(());
        };
        let Some(snapshot) = Arc::get_mut(snapshot_arc) else {
            return Err(CoreError::other(
                "application logs write failed: snapshot is still shared",
            ));
        };
        let mut key = Vec::with_capacity(1 + 32);
        key.push(prefix);
        key.extend_from_slice(&hash.to_bytes());
        let bytes = serde_json::to_vec(&value).map_err(|err| {
            CoreError::other(format!("failed to serialize application log: {err}"))
        })?;
        snapshot.put(key, bytes).map_err(|err| {
            CoreError::other(format!("failed to write application log to storage: {err}"))
        })?;
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
        match stack_values_rpc_json_per_item(&exec.stack, self.settings.max_stack_size) {
            Ok(stack) => {
                trigger.insert("stack".to_string(), Value::Array(stack));
            }
            Err(err) => {
                exception = Some(err.to_string());
            }
        }

        if include_exception || exception.is_some() {
            trigger.insert(
                "exception".to_string(),
                exception.map(Value::String).unwrap_or(Value::Null),
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
    fn blockchain_committing_handler(
        &self,
        system: &dyn Any,
        block: &Block,
        _snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
    ) {
        if self.disabled.load(Ordering::Relaxed) {
            return;
        }
        let Some(system) = system.downcast_ref::<Node>() else {
            return;
        };
        if system.settings().network != self.settings.network {
            return;
        }
        let result = panic::catch_unwind(AssertUnwindSafe(|| -> CoreResult<()> {
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
            Ok(Err(err)) => self.handle_error(&err.to_string(), "committing"),
            Err(payload) => self.handle_panic(payload, "committing"),
        }
    }
}

impl CommittedHandler for ApplicationLogsService {
    fn blockchain_committed_handler(&self, system: &dyn Any, _block: &Block) {
        if self.disabled.load(Ordering::Relaxed) {
            return;
        }
        let Some(system) = system.downcast_ref::<Node>() else {
            return;
        };
        if system.settings().network != self.settings.network {
            return;
        }
        let result = panic::catch_unwind(AssertUnwindSafe(|| self.commit_batch()));
        match result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => self.handle_error(&err.to_string(), "committed"),
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

fn vm_state_to_string(state: VMState) -> &'static str {
    match state {
        VMState::NONE => "NONE",
        VMState::HALT => "HALT",
        VMState::FAULT => "FAULT",
        VMState::BREAK => "BREAK",
    }
}

struct StackValueJsonBudget {
    remaining: isize,
}

fn stack_values_rpc_json_per_item(items: &[StackValue], max_size: usize) -> CoreResult<Vec<Value>> {
    items
        .iter()
        .map(|item| stack_value_rpc_json(item, max_size))
        .collect()
}

fn stack_value_rpc_json(item: &StackValue, max_size: usize) -> CoreResult<Value> {
    let mut budget = StackValueJsonBudget {
        remaining: isize::try_from(max_size).unwrap_or(isize::MAX),
    };
    render_stack_value(item, &mut budget)
}

fn render_stack_value(item: &StackValue, budget: &mut StackValueJsonBudget) -> CoreResult<Value> {
    let type_name = stack_value_type_name(item);
    let mut obj = Map::new();
    obj.insert("type".to_string(), Value::String(type_name.to_string()));
    budget.subtract(11 + type_name.len() as isize)?;

    let value = match item {
        StackValue::Null | StackValue::Interop(_) | StackValue::Iterator(_) => None,
        StackValue::Boolean(value) => {
            budget.subtract(if *value { 4 } else { 5 })?;
            Some(Value::Bool(*value))
        }
        StackValue::Integer(value) => {
            let text = value.to_string();
            budget.subtract(2 + text.len() as isize)?;
            Some(Value::String(text))
        }
        StackValue::BigInteger(bytes) => {
            let text = BigInt::from_signed_bytes_le(bytes).to_string();
            budget.subtract(2 + text.len() as isize)?;
            Some(Value::String(text))
        }
        StackValue::ByteString(bytes) | StackValue::Buffer(_, bytes) => {
            let encoded = BASE64_STANDARD.encode(bytes);
            budget.subtract(2 + encoded.len() as isize)?;
            Some(Value::String(encoded))
        }
        StackValue::Pointer(position) => {
            budget.subtract(position.to_string().len() as isize)?;
            Some(Value::Number(JsonNumber::from(*position)))
        }
        StackValue::Array(_, items) | StackValue::Struct(_, items) => {
            budget.subtract(2 + items.len().saturating_sub(1) as isize)?;
            let values = items
                .iter()
                .map(|entry| render_stack_value(entry, budget))
                .collect::<CoreResult<Vec<_>>>()?;
            Some(Value::Array(values))
        }
        StackValue::Map(_, entries) => {
            budget.subtract(2 + entries.len().saturating_sub(1) as isize)?;
            let values = entries
                .iter()
                .map(|(key, value)| {
                    budget.subtract(17)?;
                    let key = render_stack_value(key, budget)?;
                    let value = render_stack_value(value, budget)?;
                    let mut entry = Map::new();
                    entry.insert("key".to_string(), key);
                    entry.insert("value".to_string(), value);
                    Ok(Value::Object(entry))
                })
                .collect::<CoreResult<Vec<_>>>()?;
            Some(Value::Array(values))
        }
    };

    if let Some(value) = value {
        budget.subtract(9)?;
        obj.insert("value".to_string(), value);
    }

    budget.check()?;
    Ok(Value::Object(obj))
}

fn stack_value_type_name(item: &StackValue) -> &'static str {
    match item {
        StackValue::Null => "Any",
        StackValue::Boolean(_) => "Boolean",
        StackValue::Integer(_) | StackValue::BigInteger(_) => "Integer",
        StackValue::ByteString(_) => "ByteString",
        StackValue::Buffer(_, _) => "Buffer",
        StackValue::Array(_, _) => "Array",
        StackValue::Struct(_, _) => "Struct",
        StackValue::Map(_, _) => "Map",
        StackValue::Pointer(_) => "Pointer",
        StackValue::Interop(_) | StackValue::Iterator(_) => "InteropInterface",
    }
}

impl StackValueJsonBudget {
    fn subtract(&mut self, amount: isize) -> CoreResult<()> {
        self.remaining = self.remaining.checked_sub(amount).unwrap_or(-1);
        self.check()
    }

    fn check(&self) -> CoreResult<()> {
        if self.remaining < 0 {
            return Err(CoreError::other("Max size reached"));
        }
        Ok(())
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
        .map(|item| StackItemRpcJson::stack_item_rpc_json(item, None))
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
        fn delete(&mut self, _key: Vec<u8>) -> neo_storage::StorageResult<()> {
            Ok(())
        }

        fn put(&mut self, _key: Vec<u8>, _value: Vec<u8>) -> neo_storage::StorageResult<()> {
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
        fn delete(&mut self, _key: Vec<u8>) -> neo_storage::StorageResult<()> {
            Ok(())
        }

        fn put(&mut self, _key: Vec<u8>, _value: Vec<u8>) -> neo_storage::StorageResult<()> {
            Err(neo_storage::StorageError::invalid_operation(
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
    fn application_executed_stack_renders_from_stack_value_without_legacy_stack_item() {
        let service = ApplicationLogsService::new(
            settings(UnhandledExceptionPolicy::Ignore),
            Arc::new(FailingStore),
        );
        let exec = ApplicationExecuted::new(
            None,
            TriggerType::APPLICATION,
            VMState::HALT,
            None,
            0,
            vec![StackValue::Struct(vec![
                StackValue::Integer(1),
                StackValue::ByteString(b"neo".to_vec()),
            ])],
        );

        let json = service.transaction_log_json(&UInt256::zero(), &exec);
        let stack = json["executions"][0]["stack"]
            .as_array()
            .expect("stack array");

        assert_eq!(stack[0]["type"], Value::String("Struct".to_string()));
        let fields = stack[0]["value"].as_array().expect("struct fields");
        assert_eq!(fields[0]["type"], Value::String("Integer".to_string()));
        assert_eq!(fields[0]["value"], Value::String("1".to_string()));
        assert_eq!(fields[1]["type"], Value::String("ByteString".to_string()));
        assert_eq!(fields[1]["value"], Value::String("bmVv".to_string()));
    }

    #[test]
    fn application_executed_stack_value_renderer_preserves_csharp_max_size_error() {
        let mut settings = settings(UnhandledExceptionPolicy::Ignore);
        settings.max_stack_size = 4;
        let service = ApplicationLogsService::new(settings, Arc::new(FailingStore));
        let exec = ApplicationExecuted::new(
            None,
            TriggerType::APPLICATION,
            VMState::HALT,
            None,
            0,
            vec![StackValue::ByteString(vec![0xaa; 16])],
        );

        let json = service.transaction_log_json(&UInt256::zero(), &exec);
        let execution = &json["executions"][0];

        assert_eq!(
            execution["exception"],
            Value::String("Max size reached".to_string())
        );
        assert!(execution.get("stack").is_none());
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

        assert!(
            err.to_string()
                .contains("injected application logs commit failure")
        );
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

        assert!(
            err.to_string()
                .contains("injected application logs write failure")
        );
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
}
