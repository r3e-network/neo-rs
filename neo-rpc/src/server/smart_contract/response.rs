//! Response projection helpers for smart-contract RPC handlers.
//!
//! This module owns the VM-facing JSON shapes shared by invoke, contract
//! verification, and iterator-session handlers. Request parsing remains in the
//! request/helper modules.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_payloads::NotifyEventArgs;
use neo_vm::VmState;
use neo_vm::rpc_json::StackItemRpcJson;
use neo_vm::stack_item::StackItem;
use serde_json::{Map, Value, json};

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::session::Session;

const INVALID_OPERATION_CODE: i32 = -2146233079;

pub(super) fn invoke_result_base_to_json(
    script: &[u8],
    state: String,
    gas_consumed: impl ToString,
    exception: Value,
) -> Map<String, Value> {
    let mut result = Map::new();
    result.insert(
        "script".to_string(),
        Value::String(BASE64_STANDARD.encode(script)),
    );
    result.insert("state".to_string(), Value::String(state));
    result.insert(
        "gasconsumed".to_string(),
        Value::String(gas_consumed.to_string()),
    );
    result.insert("exception".to_string(), exception);
    result
}

pub(super) fn insert_notifications(result: &mut Map<String, Value>, notifications: Vec<Value>) {
    result.insert("notifications".to_string(), Value::Array(notifications));
}

pub(super) fn insert_stack(result: &mut Map<String, Value>, stack_items: Vec<Value>) {
    result.insert("stack".to_string(), Value::Array(stack_items));
}

pub(super) fn insert_diagnostics(
    result: &mut Map<String, Value>,
    invocation: Value,
    storage: Value,
) {
    result.insert(
        "diagnostics".to_string(),
        json!({
            "invokedcontracts": invocation,
            "storagechanges": storage,
        }),
    );
}

pub(super) fn insert_session(result: &mut Map<String, Value>, session_id: impl ToString) {
    result.insert("session".to_string(), Value::String(session_id.to_string()));
}

pub(super) fn iterator_values_to_json(values: Vec<Value>) -> Value {
    Value::Array(values)
}

pub(super) fn terminate_session_to_json(terminated: bool) -> Value {
    Value::Bool(terminated)
}

pub(super) fn final_rpc_vm_state_string(state: VmState) -> Result<String, RpcException> {
    state
        .final_name()
        .map(str::to_string)
        .ok_or_else(|| internal_error(format!("{state:?} is not a final VM state")))
}

pub(super) fn stack_item_to_json(
    item: &StackItem,
    session: Option<&mut Session>,
) -> Result<Value, RpcException> {
    stack_item_to_json_with_budget(item, session, None)
}

pub(super) fn stack_item_to_json_limited(
    item: &StackItem,
    session: Option<&mut Session>,
    max_size: usize,
) -> Result<Value, RpcException> {
    stack_item_to_json_with_budget(item, session, Some(max_size))
}

fn stack_item_to_json_with_budget(
    item: &StackItem,
    session: Option<&mut Session>,
    max_size: Option<usize>,
) -> Result<Value, RpcException> {
    let mut value = StackItemRpcJson::stack_item_rpc_json_deferred_size_check(item, max_size)
        .map_err(|err| stack_item_error(err.to_string()))?;
    if let StackItem::InteropInterface(iface) = item {
        if let Some(session) = session {
            if let Some(iterator_id) = session.register_iterator_interface(iface) {
                if let Value::Object(obj) = &mut value {
                    obj.insert(
                        // C# `RpcServer.SmartContract` emits `nameof(IIterator)` =
                        // the literal "IIterator" for an iterator stack item.
                        "interface".to_string(),
                        Value::String("IIterator".to_string()),
                    );
                    obj.insert("id".to_string(), Value::String(iterator_id.to_string()));
                }
            }
        }
    }
    Ok(value)
}

fn stack_item_error(message: impl Into<String>) -> RpcException {
    RpcException::new(INVALID_OPERATION_CODE, message.into())
}

pub(super) fn notification_to_json(
    notification: &NotifyEventArgs,
    mut session: Option<&mut Session>,
) -> Result<Value, RpcException> {
    let mut state = Vec::new();
    for entry in notification.state() {
        state.push(stack_item_to_json(entry, session.as_deref_mut())?);
    }
    Ok(json!({
        "eventname": notification.event_name,
        "contract": notification.script_hash.to_string(),
        "state": state}))
}

pub(super) fn unclaimed_gas_to_json(address: String, unclaimed_datoshi: impl ToString) -> Value {
    json!({
        "address": address,
        // C# GetUnclaimedGas returns the raw datoshi BigInteger as a string
        // (NEO.UnclaimedGas(...).ToString()), e.g. "100000000" for 1 GAS - not
        // the decimal form. Wrapping in BigDecimal would divide by 10^8.
        "unclaimed": unclaimed_datoshi.to_string(),
    })
}
