use alloc::{string::String, vec::Vec};

#[cfg(feature = "std")]
use serde_json::json;

use neo_base::Bytes;
use neo_vm::VmValue;

use crate::manifest::ParameterKind;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Bytes(Bytes),
    String(String),
    Sequence(Vec<Value>),
}

impl Value {
    pub fn kind(&self) -> ParameterKind {
        match self {
            Value::Null => ParameterKind::ByteArray,
            Value::Bool(_) => ParameterKind::Boolean,
            Value::Int(_) => ParameterKind::Integer,
            Value::Bytes(_) => ParameterKind::ByteArray,
            Value::String(_) => ParameterKind::String,
            Value::Sequence(_) => ParameterKind::Array,
        }
    }

    #[cfg(feature = "std")]
    pub fn to_stack_json(&self) -> serde_json::Value {
        match self {
            Value::Null => json!({ "type": "Any", "value": serde_json::Value::Null }),
            Value::Bool(v) => json!({ "type": "Boolean", "value": v }),
            Value::Int(v) => json!({ "type": "Integer", "value": v.to_string() }),
            Value::Bytes(bytes) => json!({
                "type": "ByteString",
                "value": hex::encode(bytes.as_slice())
            }),
            Value::String(value) => json!({ "type": "String", "value": value }),
            Value::Sequence(values) => json!({
                "type": "Array",
                "value": values.iter().map(|v| v.to_stack_json()).collect::<Vec<_>>()
            }),
        }
    }
}

impl From<VmValue> for Value {
    fn from(value: VmValue) -> Self {
        match value {
            VmValue::Null => Value::Null,
            VmValue::Bool(v) => Value::Bool(v),
            VmValue::Int(v) => Value::Int(v),
            VmValue::Bytes(bytes) => Value::Bytes(bytes),
            VmValue::String(s) => Value::String(s),
            VmValue::Array(values) => {
                Value::Sequence(values.into_iter().map(Value::from).collect())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InvocationResult {
    pub value: Value,
    pub gas_used: u64,
    pub logs: Vec<String>,
    pub notifications: Vec<(String, Vec<Value>)>,
}

impl InvocationResult {
    pub fn new(value: Value, gas_used: u64) -> Self {
        Self {
            value,
            gas_used,
            logs: Vec::new(),
            notifications: Vec::new(),
        }
    }

    pub fn with_events(
        mut self,
        logs: Vec<String>,
        notifications: Vec<(String, Vec<Value>)>,
    ) -> Self {
        self.logs = logs;
        self.notifications = notifications;
        self
    }
}
