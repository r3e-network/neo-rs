use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::find_options::FindOptions;
use neo_core::smart_contract::iterators::IIterator;
use neo_core::smart_contract::storage_context::StorageContext;
use neo_core::smart_contract::storage_item::StorageItem;
use neo_core::smart_contract::storage_key::StorageKey;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_vm::StackItem;
use std::sync::Arc;

#[test]
fn storage_find_values_only_returns_payload() {
    let snapshot = Arc::new(DataCache::new(false));
    let contract_id = 0x43;

    let storage_key = StorageKey::new(contract_id, vec![0x01]);
    let storage_item = StorageItem::from_bytes(vec![0x01, 0x02, 0x03, 0x04]);
    snapshot.add(storage_key, storage_item.clone());

    let engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        ProtocolSettings::default(),
        200_000_000,
        None,
    )
    .expect("engine");

    let context = StorageContext::new(contract_id, false);
    let mut iterator = engine
        .find_storage_entries(&context, &[0x01], FindOptions::ValuesOnly)
        .expect("find storage");

    assert!(iterator.next());
    let StackItem::ByteString(value) = iterator.value() else {
        panic!("expected byte string");
    };
    assert_eq!(value, storage_item.get_value());
}
