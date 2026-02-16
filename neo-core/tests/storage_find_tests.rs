use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::binary_serializer::BinarySerializer;
use neo_core::smart_contract::find_options::FindOptions;
use neo_core::smart_contract::iterators::IIterator;
use neo_core::smart_contract::storage_context::StorageContext;
use neo_core::smart_contract::storage_item::StorageItem;
use neo_core::smart_contract::storage_key::StorageKey;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_vm::StackItem;
use neo_vm::execution_engine_limits::ExecutionEngineLimits;
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

#[test]
fn storage_find_keys_only_can_remove_prefix() {
    let snapshot = Arc::new(DataCache::new(false));
    let contract_id = 0x43;
    let prefix = vec![0x10];

    snapshot.add(
        StorageKey::new(contract_id, vec![0x10, 0xAA]),
        StorageItem::from_bytes(vec![0x01]),
    );

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
        .find_storage_entries(
            &context,
            &prefix,
            FindOptions::KeysOnly | FindOptions::RemovePrefix,
        )
        .expect("find storage");

    assert!(iterator.next());
    let StackItem::ByteString(key) = iterator.value() else {
        panic!("expected byte string key");
    };
    assert_eq!(key, vec![0xAA]);
}

#[test]
fn storage_find_backwards_orders_descending() {
    let snapshot = Arc::new(DataCache::new(false));
    let contract_id = 0x43;
    let prefix = vec![0x22];

    snapshot.add(
        StorageKey::new(contract_id, vec![0x22, 0x01]),
        StorageItem::from_bytes(vec![0x01]),
    );
    snapshot.add(
        StorageKey::new(contract_id, vec![0x22, 0x02]),
        StorageItem::from_bytes(vec![0x02]),
    );

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
        .find_storage_entries(
            &context,
            &prefix,
            FindOptions::KeysOnly | FindOptions::RemovePrefix | FindOptions::Backwards,
        )
        .expect("find storage");

    assert!(iterator.next());
    let StackItem::ByteString(first) = iterator.value() else {
        panic!("expected byte string key");
    };
    assert_eq!(first, vec![0x02]);
    assert!(iterator.next());
    let StackItem::ByteString(second) = iterator.value() else {
        panic!("expected byte string key");
    };
    assert_eq!(second, vec![0x01]);
}

#[test]
fn storage_find_deserialize_values_pick_field0() {
    let snapshot = Arc::new(DataCache::new(false));
    let contract_id = 0x43;

    let value_item = StackItem::from_array(vec![
        StackItem::from_byte_string(vec![0xAA]),
        StackItem::from_byte_string(vec![0xBB]),
    ]);
    let serialized = BinarySerializer::serialize(&value_item, &ExecutionEngineLimits::default())
        .expect("serialize");

    snapshot.add(
        StorageKey::new(contract_id, vec![0x01]),
        StorageItem::from_bytes(serialized),
    );

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
        .find_storage_entries(
            &context,
            &[0x01],
            FindOptions::ValuesOnly | FindOptions::DeserializeValues | FindOptions::PickField0,
        )
        .expect("find storage");

    assert!(iterator.next());
    let StackItem::ByteString(value) = iterator.value() else {
        panic!("expected byte string value");
    };
    assert_eq!(value, vec![0xAA]);
}
