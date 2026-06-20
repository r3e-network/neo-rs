use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_error::{CoreError, CoreResult};
use neo_vm_rs::StackValue;
use num_bigint::BigInt;
use serde_json::{Map, Number as JsonNumber, Value};

pub(super) fn stack_values_rpc_json_per_item(
    items: &[StackValue],
    max_size: usize,
) -> CoreResult<Vec<Value>> {
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

struct StackValueJsonBudget {
    remaining: isize,
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
