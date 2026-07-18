use super::StackItemRpcJson;
use crate::StackItem;
use crate::VmOrderedDictionary;
use crate::script::Script;
use crate::stack_item::InteropInterface;
use serde_json::json;
use std::sync::Arc;

#[test]
fn renders_rpc_stack_item_type_matrix() {
    let mut map = VmOrderedDictionary::new();
    map.insert(
        StackItem::from_byte_string(b"k".to_vec()),
        StackItem::from_i64(9),
    );

    let cases = vec![
        (StackItem::Null, json!({"type": "Any"})),
        (
            StackItem::Boolean(true),
            json!({"type": "Boolean", "value": true}),
        ),
        (
            StackItem::from_i64(42),
            json!({"type": "Integer", "value": "42"}),
        ),
        (
            StackItem::from_byte_string(vec![1, 2]),
            json!({"type": "ByteString", "value": "AQI="}),
        ),
        (
            StackItem::from_buffer(vec![3, 4]),
            json!({"type": "Buffer", "value": "AwQ="}),
        ),
        (
            StackItem::from_pointer(Arc::new(Script::new_from_bytes(vec![])), 7),
            json!({"type": "Pointer", "value": 7}),
        ),
        (
            StackItem::from_array(vec![StackItem::Boolean(false)]),
            json!({"type": "Array", "value": [
                {"type": "Boolean", "value": false}
            ]}),
        ),
        (
            StackItem::from_struct(vec![StackItem::from_i64(1)]),
            json!({"type": "Struct", "value": [
                {"type": "Integer", "value": "1"}
            ]}),
        ),
        (
            StackItem::from_map(map),
            json!({"type": "Map", "value": [{
                "key": {"type": "ByteString", "value": "aw=="},
                "value": {"type": "Integer", "value": "9"}
            }]}),
        ),
        (
            StackItem::from_interface(InteropInterface::iterator(1)),
            json!({"type": "InteropInterface"}),
        ),
    ];

    for (item, expected) in cases {
        assert_eq!(
            StackItemRpcJson::stack_item_rpc_json(&item, None).unwrap(),
            expected
        );
    }
}

#[test]
fn applies_size_budget_per_top_level_item() {
    let items = vec![StackItem::Null, StackItem::Null];
    let values = StackItemRpcJson::stack_items_rpc_json_per_item(&items, 14).unwrap();

    assert_eq!(values, vec![json!({"type": "Any"}), json!({"type": "Any"})]);
}

#[test]
fn reports_max_size_reached() {
    let err = StackItemRpcJson::stack_item_rpc_json(&StackItem::Null, Some(13)).unwrap_err();

    assert!(err.to_string().contains("Max size reached"));
}

#[test]
fn reports_circular_reference() {
    let item = StackItem::from_array(vec![StackItem::Null]);
    if let StackItem::Array(array) = &item {
        let _ = array.set(0, item.clone());
    }

    let err = StackItemRpcJson::stack_item_rpc_json(&item, None).unwrap_err();

    assert!(err.to_string().contains("Circular reference"));
}

#[test]
fn deferred_size_check_preserves_rpc_circular_reference_precedence() {
    let item = StackItem::from_array(vec![StackItem::Null]);
    if let StackItem::Array(array) = &item {
        let _ = array.set(0, item.clone());
    }

    let err =
        StackItemRpcJson::stack_item_rpc_json_deferred_size_check(&item, Some(1)).unwrap_err();

    assert!(err.to_string().contains("Circular reference"));
}

#[test]
fn deferred_size_check_still_reports_max_size() {
    let err = StackItemRpcJson::stack_item_rpc_json_deferred_size_check(&StackItem::Null, Some(13))
        .unwrap_err();

    assert!(err.to_string().contains("Max size reached"));
}
