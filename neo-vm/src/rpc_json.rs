//! JSON-RPC envelope rendering for host VM stack items.

use crate::StackItem;
use crate::error::VmError;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_vm_rs::StackItemType;
use serde_json::{Map, Number as JsonNumber, Value};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SizeCheck {
    Immediate,
    Deferred,
}

struct RenderBudget {
    remaining: isize,
    size_check: SizeCheck,
}

/// Renders a single stack item as the Neo JSON-RPC stack envelope.
pub fn stack_item_rpc_json(item: &StackItem, max_size: Option<usize>) -> Result<Value, VmError> {
    render_stack_item_with_size_check(item, max_size, SizeCheck::Immediate)
}

/// Renders with the legacy RPC budget timing.
///
/// The RPC server historically checked the remaining budget after each item was
/// fully rendered, so a circular reference discovered during traversal takes
/// precedence over an already-exhausted size budget.
pub fn stack_item_rpc_json_deferred_size_check(
    item: &StackItem,
    max_size: Option<usize>,
) -> Result<Value, VmError> {
    render_stack_item_with_size_check(item, max_size, SizeCheck::Deferred)
}

/// Renders top-level stack items with an independent size budget for each item.
pub fn stack_items_rpc_json_per_item(
    items: &[StackItem],
    max_size: usize,
) -> Result<Vec<Value>, VmError> {
    items
        .iter()
        .map(|item| stack_item_rpc_json(item, Some(max_size)))
        .collect()
}

fn render_stack_item_with_size_check(
    item: &StackItem,
    max_size: Option<usize>,
    size_check: SizeCheck,
) -> Result<Value, VmError> {
    let mut context = HashSet::new();
    let mut budget = RenderBudget {
        remaining: max_size
            .and_then(|value| isize::try_from(value).ok())
            .unwrap_or(isize::MAX),
        size_check,
    };
    render_stack_item(item, &mut context, &mut budget)
}

fn render_stack_item(
    item: &StackItem,
    context: &mut HashSet<(usize, StackItemType)>,
    budget: &mut RenderBudget,
) -> Result<Value, VmError> {
    let type_name = stack_item_type_name(item);
    let mut obj = Map::new();
    obj.insert("type".to_string(), Value::String(type_name.to_string()));
    budget.subtract(11 + type_name.len() as isize)?;

    let mut value = None;
    match item {
        StackItem::Null | StackItem::InteropInterface(_) => {}
        StackItem::Boolean(flag) => {
            budget.subtract(if *flag { 4 } else { 5 })?;
            value = Some(Value::Bool(*flag));
        }
        StackItem::Integer(integer) => {
            let text = integer.to_string();
            budget.subtract(2 + text.len() as isize)?;
            value = Some(Value::String(text));
        }
        StackItem::ByteString(bytes) => {
            let encoded = BASE64_STANDARD.encode(bytes);
            budget.subtract(2 + encoded.len() as isize)?;
            value = Some(Value::String(encoded));
        }
        StackItem::Buffer(buffer) => {
            let encoded = BASE64_STANDARD.encode(buffer.data());
            budget.subtract(2 + encoded.len() as isize)?;
            value = Some(Value::String(encoded));
        }
        StackItem::Pointer(pointer) => {
            let position = pointer.position() as u64;
            budget.subtract(position.to_string().len() as isize)?;
            value = Some(Value::Number(JsonNumber::from(position)));
        }
        StackItem::Array(array) => {
            let identity = (array.id(), StackItemType::Array);
            if !context.insert(identity) {
                return Err(VmError::invalid_operation_msg(
                    "Circular reference in stack item",
                ));
            }
            budget.subtract(2 + array.len().saturating_sub(1) as isize)?;
            let values = array
                .iter()
                .map(|entry| render_stack_item(&entry, context, budget))
                .collect::<Result<Vec<_>, _>>()?;
            context.remove(&identity);
            value = Some(Value::Array(values));
        }
        StackItem::Struct(structure) => {
            let identity = (structure.id(), StackItemType::Struct);
            if !context.insert(identity) {
                return Err(VmError::invalid_operation_msg(
                    "Circular reference in stack item",
                ));
            }
            budget.subtract(2 + structure.len().saturating_sub(1) as isize)?;
            let values = structure
                .iter()
                .map(|entry| render_stack_item(&entry, context, budget))
                .collect::<Result<Vec<_>, _>>()?;
            context.remove(&identity);
            value = Some(Value::Array(values));
        }
        StackItem::Map(map) => {
            let identity = (map.id(), StackItemType::Map);
            if !context.insert(identity) {
                return Err(VmError::invalid_operation_msg(
                    "Circular reference in stack item",
                ));
            }
            budget.subtract(2 + map.len().saturating_sub(1) as isize)?;
            let values = map
                .iter()
                .map(|(key, value)| {
                    budget.subtract(17)?;
                    let key = render_stack_item(&key, context, budget)?;
                    let value = render_stack_item(&value, context, budget)?;
                    let mut entry = Map::new();
                    entry.insert("key".to_string(), key);
                    entry.insert("value".to_string(), value);
                    Ok(Value::Object(entry))
                })
                .collect::<Result<Vec<_>, VmError>>()?;
            context.remove(&identity);
            value = Some(Value::Array(values));
        }
    }

    if let Some(value) = value {
        budget.subtract(9)?;
        obj.insert("value".to_string(), value);
    }

    budget.check()?;
    Ok(Value::Object(obj))
}

const fn stack_item_type_name(item: &StackItem) -> &'static str {
    match item {
        StackItem::Null => "Any",
        StackItem::Boolean(_) => "Boolean",
        StackItem::Integer(_) => "Integer",
        StackItem::ByteString(_) => "ByteString",
        StackItem::Buffer(_) => "Buffer",
        StackItem::Array(_) => "Array",
        StackItem::Struct(_) => "Struct",
        StackItem::Map(_) => "Map",
        StackItem::Pointer(_) => "Pointer",
        StackItem::InteropInterface(_) => "InteropInterface",
    }
}

impl RenderBudget {
    fn subtract(&mut self, amount: isize) -> Result<(), VmError> {
        self.remaining = self.remaining.checked_sub(amount).unwrap_or(-1);
        if self.size_check == SizeCheck::Immediate {
            self.check()?;
        }
        Ok(())
    }

    fn check(&self) -> Result<(), VmError> {
        if self.remaining < 0 {
            return Err(VmError::invalid_operation_msg("Max size reached"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        stack_item_rpc_json, stack_item_rpc_json_deferred_size_check, stack_items_rpc_json_per_item,
    };
    use crate::StackItem;
    use crate::script::Script;
    use crate::stack_item::InteropInterface;
    use neo_vm_rs::VmOrderedDictionary;
    use serde_json::json;
    use std::sync::Arc;

    #[derive(Debug)]
    struct DummyInterop;

    impl InteropInterface for DummyInterop {
        fn interface_type(&self) -> &str {
            "Dummy"
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

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
                StackItem::from_interface(DummyInterop),
                json!({"type": "InteropInterface"}),
            ),
        ];

        for (item, expected) in cases {
            assert_eq!(stack_item_rpc_json(&item, None).unwrap(), expected);
        }
    }

    #[test]
    fn applies_size_budget_per_top_level_item() {
        let items = vec![StackItem::Null, StackItem::Null];
        let values = stack_items_rpc_json_per_item(&items, 14).unwrap();

        assert_eq!(values, vec![json!({"type": "Any"}), json!({"type": "Any"})]);
    }

    #[test]
    fn reports_max_size_reached() {
        let err = stack_item_rpc_json(&StackItem::Null, Some(13)).unwrap_err();

        assert!(err.to_string().contains("Max size reached"));
    }

    #[test]
    fn reports_circular_reference() {
        let item = StackItem::from_array(vec![StackItem::Null]);
        if let StackItem::Array(array) = &item {
            let _ = array.set(0, item.clone());
        }

        let err = stack_item_rpc_json(&item, None).unwrap_err();

        assert!(err.to_string().contains("Circular reference"));
    }

    #[test]
    fn deferred_size_check_preserves_rpc_circular_reference_precedence() {
        let item = StackItem::from_array(vec![StackItem::Null]);
        if let StackItem::Array(array) = &item {
            let _ = array.set(0, item.clone());
        }

        let err = stack_item_rpc_json_deferred_size_check(&item, Some(1)).unwrap_err();

        assert!(err.to_string().contains("Circular reference"));
    }

    #[test]
    fn deferred_size_check_still_reports_max_size() {
        let err = stack_item_rpc_json_deferred_size_check(&StackItem::Null, Some(13)).unwrap_err();

        assert!(err.to_string().contains("Max size reached"));
    }
}
