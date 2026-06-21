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

/// JSON-RPC envelope rendering for host VM stack items.
pub struct StackItemRpcJson;

impl StackItemRpcJson {
    /// Renders a single stack item as the Neo JSON-RPC stack envelope.
    pub fn stack_item_rpc_json(
        item: &StackItem,
        max_size: Option<usize>,
    ) -> Result<Value, VmError> {
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
            .map(|item| Self::stack_item_rpc_json(item, Some(max_size)))
            .collect()
    }
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
#[path = "tests/rpc_json.rs"]
mod tests;
