use super::*;
use neo_config::ProtocolSettings;
use neo_primitives::FindOptions;
use neo_storage::{StorageItem, StorageKey};
use neo_vm_rs::OpCode;

#[tokio::test(flavor = "multi_thread")]
async fn session_registers_and_traverses_storage_iterator() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings);
    let session = Session::new(
        system.clone(), // Arc<Node> coerced to Arc<dyn StoreProvider>
        system,         // Arc<Node> coerced to Arc<dyn ConfigProvider>
        vec![OpCode::RET.byte()],
        None,
        None,
        100_000_000,
        None,
    )
    .expect("session");

    let entries = vec![
        (
            StorageKey::new(1, vec![0x01]),
            StorageItem::from_bytes(vec![0xAA]),
        ),
        (
            StorageKey::new(1, vec![0x02]),
            StorageItem::from_bytes(vec![0xBB]),
        ),
    ];
    let iterator = StorageIterator::new(entries, 0, FindOptions::None);
    let iterator_id = {
        let mut engine = session.engine_mut();
        engine
            .store_storage_iterator(iterator)
            .expect("store iterator")
    };

    let interop = Arc::new(IteratorInterop::new(iterator_id)) as Arc<dyn VmInteropInterface>;
    let uuid_first = session
        .register_iterator_interface(&interop)
        .expect("iterator registered");
    let uuid_second = session
        .register_iterator_interface(&interop)
        .expect("iterator re-registered");
    assert_eq!(uuid_first, uuid_second);
    assert!(session.has_iterators());

    let values = session
        .traverse_iterator(&uuid_first, 10)
        .expect("traverse iterator");
    assert_eq!(values.len(), 2);
    assert!(matches!(values[0], StackItem::Struct(_)));

    let tail = session
        .traverse_iterator(&uuid_first, 10)
        .expect("traverse iterator exhausted");
    assert!(tail.is_empty());
}
