use super::*;
use neo_primitives::TriggerType;
use neo_primitives::UnhandledExceptionPolicy;
use neo_storage::persistence::{
    SeekDirection,
    read_only_store::{RawReadOnlyStore, ReadOnlyStore, ReadOnlyStoreGeneric},
    storage::StorageError,
    store::OnNewSnapshotDelegate,
    write_store::WriteStore,
};
use neo_storage::{StorageItem, StorageKey};
use neo_vm_rs::{StackValue, VmState as VMState};

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

impl RawReadOnlyStore for FailingStore {
    fn try_get_bytes(&self, _key: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

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

impl RawReadOnlyStore for FailingSnapshot {
    fn try_get_bytes(&self, _key: &[u8]) -> Option<Vec<u8>> {
        None
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
        vec![StackValue::Struct(
            neo_vm_rs::next_stack_item_id(),
            vec![
                StackValue::Integer(1),
                StackValue::ByteString(b"neo".to_vec()),
            ],
        )],
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
