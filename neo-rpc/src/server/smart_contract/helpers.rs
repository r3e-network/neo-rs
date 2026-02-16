use std::collections::HashSet;
use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_core::UInt160;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::smart_contract::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::contract_parameter::{ContractParameter, ContractParameterValue};
use neo_core::smart_contract::notify_event_args::NotifyEventArgs;
use neo_json::JToken;
use parking_lot::Mutex;
use serde_json::{Map, Number as JsonNumber, Value, json};
use uuid::Uuid;

use crate::server::diagnostic::Diagnostic;
use crate::server::model::signers_and_witnesses::SignersAndWitnesses;
use crate::server::parameter_converter::{ConversionContext, ParameterConverter};
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;
use crate::server::session::Session;
use crate::server::tree_node::TreeNode;

use neo_vm::OrderedDictionary;
use neo_vm::stack_item::StackItem;

const INVALID_OPERATION_CODE: i32 = -2146233079;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
enum CompoundIdentity {
    Array(usize),
    Struct(usize),
    Map(usize),
}

pub(super) fn parse_contract_parameters(
    arg: Option<&Value>,
) -> Result<Vec<ContractParameter>, RpcException> {
    match arg {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| ContractParameter::from_json(value).map_err(invalid_params))
            .collect(),
        Some(_) => Err(invalid_params("args must be an array")),
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn parse_signers_and_witnesses(
    server: &RpcServer,
    value: Option<&Value>,
) -> Result<(Option<Vec<Signer>>, Option<Vec<Witness>>), RpcException> {
    let Some(token_value) = value else {
        return Ok((None, None));
    };
    let jtoken: JToken = serde_json::from_value(token_value.clone())
        .map_err(|err| invalid_params(err.to_string()))?;
    let ctx = ConversionContext::new(server.system().settings().address_version);
    let parsed = ParameterConverter::convert::<SignersAndWitnesses>(&jtoken, &ctx)?;
    let signers = if parsed.signers().is_empty() {
        None
    } else {
        Some(parsed.signers().to_vec())
    };
    let witnesses = if parsed.witnesses().is_empty() {
        None
    } else {
        Some(parsed.witnesses().to_vec())
    };
    Ok((signers, witnesses))
}

pub(super) fn build_dynamic_call_script(
    script_hash: UInt160,
    operation: &str,
    parameters: &[ContractParameter],
) -> Result<Vec<u8>, RpcException> {
    let args = parameters
        .iter()
        .map(contract_parameter_to_stack_item)
        .collect::<Result<Vec<_>, _>>()?;
    let mut builder = neo_vm::script_builder::ScriptBuilder::new();

    if args.is_empty() {
        builder.emit_opcode(neo_vm::op_code::OpCode::NEWARRAY0);
    } else {
        for item in args.iter().rev() {
            builder
                .emit_push_stack_item(item.clone())
                .map_err(|err| internal_error(err.to_string()))?;
        }
        builder.emit_push_int(args.len() as i64);
        builder.emit_opcode(neo_vm::op_code::OpCode::PACK);
    }

    builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    builder.emit_push(operation.as_bytes());
    builder.emit_push(script_hash.to_bytes().as_ref());
    builder
        .emit_syscall("System.Contract.Call")
        .map_err(|err| internal_error(err.to_string()))?;

    Ok(builder.to_array())
}

pub(super) fn contract_parameter_to_stack_item(
    parameter: &ContractParameter,
) -> Result<StackItem, RpcException> {
    match &parameter.value {
        ContractParameterValue::Any | ContractParameterValue::Void => Ok(StackItem::Null),
        ContractParameterValue::Boolean(value) => Ok(StackItem::from_bool(*value)),
        ContractParameterValue::Integer(value) => Ok(StackItem::from_int(value.clone())),
        ContractParameterValue::Hash160(value) => Ok(StackItem::from_byte_string(value.to_bytes())),
        ContractParameterValue::Hash256(value) => {
            Ok(StackItem::from_byte_string(value.to_array().to_vec()))
        }
        ContractParameterValue::ByteArray(bytes) | ContractParameterValue::Signature(bytes) => {
            Ok(StackItem::from_byte_string(bytes.clone()))
        }
        ContractParameterValue::PublicKey(point) => {
            Ok(StackItem::from_byte_string(point.encoded()))
        }
        ContractParameterValue::String(value) => {
            Ok(StackItem::from_byte_string(value.as_bytes().to_vec()))
        }
        ContractParameterValue::Array(items) => {
            let converted = items
                .iter()
                .map(contract_parameter_to_stack_item)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(StackItem::from_array(converted))
        }
        ContractParameterValue::Map(entries) => {
            #[allow(clippy::mutable_key_type)]
            let mut map = OrderedDictionary::new();
            for (key, value) in entries {
                let key_item = contract_parameter_to_stack_item(key)?;
                let value_item = contract_parameter_to_stack_item(value)?;
                map.insert(key_item, value_item);
            }
            Ok(StackItem::from_map(map))
        }
        ContractParameterValue::InteropInterface => Err(invalid_params(
            "InteropInterface parameters are not supported in invoke RPCs",
        )),
    }
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
    let mut context = HashSet::new();
    let mut remaining = max_size
        .and_then(|value| i64::try_from(value).ok())
        .unwrap_or(i64::MAX);
    let mut value = stack_item_to_json_inner(item, &mut context, &mut remaining)?;
    if let StackItem::InteropInterface(iface) = item {
        if let Some(session) = session {
            if let Some(iterator_id) = session.register_iterator_interface(iface) {
                if let Value::Object(obj) = &mut value {
                    obj.insert(
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

fn stack_item_to_json_inner(
    item: &StackItem,
    context: &mut HashSet<CompoundIdentity>,
    remaining: &mut i64,
) -> Result<Value, RpcException> {
    let type_name = stack_item_type_name(item);
    let mut obj = Map::new();
    obj.insert("type".to_string(), Value::String(type_name.to_string()));
    *remaining -= 11 + type_name.len() as i64;

    let mut value = None;
    match item {
        StackItem::Null | StackItem::InteropInterface(_) => {}
        StackItem::Boolean(flag) => {
            *remaining -= if *flag { 4 } else { 5 };
            value = Some(Value::Bool(*flag));
        }
        StackItem::Integer(integer) => {
            let text = integer.to_string();
            *remaining -= 2 + text.len() as i64;
            value = Some(Value::String(text));
        }
        StackItem::ByteString(bytes) => {
            let encoded = BASE64_STANDARD.encode(bytes);
            *remaining -= 2 + encoded.len() as i64;
            value = Some(Value::String(encoded));
        }
        StackItem::Buffer(buffer) => {
            let encoded = BASE64_STANDARD.encode(buffer.data());
            *remaining -= 2 + encoded.len() as i64;
            value = Some(Value::String(encoded));
        }
        StackItem::Array(array) => {
            let identity = CompoundIdentity::Array(array.id());
            if !context.insert(identity) {
                return Err(stack_item_error("Circular reference."));
            }
            let items = array.items();
            let count = items.len() as i64;
            *remaining -= 2 + count.saturating_sub(1);
            let values = items
                .iter()
                .map(|entry| stack_item_to_json_inner(entry, context, remaining))
                .collect::<Result<Vec<_>, _>>()?;
            if !context.remove(&identity) {
                return Err(stack_item_error("Circular reference."));
            }
            value = Some(Value::Array(values));
        }
        StackItem::Struct(structure) => {
            let identity = CompoundIdentity::Struct(structure.id());
            if !context.insert(identity) {
                return Err(stack_item_error("Circular reference."));
            }
            let items = structure.items();
            let count = items.len() as i64;
            *remaining -= 2 + count.saturating_sub(1);
            let values = items
                .iter()
                .map(|entry| stack_item_to_json_inner(entry, context, remaining))
                .collect::<Result<Vec<_>, _>>()?;
            if !context.remove(&identity) {
                return Err(stack_item_error("Circular reference."));
            }
            value = Some(Value::Array(values));
        }
        StackItem::Map(map) => {
            let identity = CompoundIdentity::Map(map.id());
            if !context.insert(identity) {
                return Err(stack_item_error("Circular reference."));
            }
            let count = map.len() as i64;
            *remaining -= 2 + count.saturating_sub(1);
            let entries = map
                .iter()
                .map(|(key, value)| {
                    *remaining -= 17;
                    let key_json = stack_item_to_json_inner(&key, context, remaining)?;
                    let value_json = stack_item_to_json_inner(&value, context, remaining)?;
                    Ok(json!({
                        "key": key_json,
                        "value": value_json,
                    }))
                })
                .collect::<Result<Vec<_>, RpcException>>()?;
            if !context.remove(&identity) {
                return Err(stack_item_error("Circular reference."));
            }
            value = Some(Value::Array(entries));
        }
        StackItem::Pointer(pointer) => {
            let position = pointer.position();
            let position_text = position.to_string();
            *remaining -= position_text.len() as i64;
            value = Some(Value::Number(JsonNumber::from(position as u64)));
        }
    }

    if let Some(value) = value {
        *remaining -= 9;
        obj.insert("value".to_string(), value);
    }

    if *remaining < 0 {
        return Err(stack_item_error("Max size reached."));
    }

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

fn stack_item_error(message: impl Into<String>) -> RpcException {
    RpcException::new(INVALID_OPERATION_CODE, message.into())
}

pub(super) fn notification_to_json(
    notification: &NotifyEventArgs,
    mut session: Option<&mut Session>,
) -> Result<Value, RpcException> {
    let mut state = Vec::new();
    for entry in &notification.state {
        state.push(stack_item_to_json(entry, session.as_deref_mut())?);
    }
    Ok(json!({
        "eventname": notification.event_name,
        "contract": notification.script_hash.to_string(),
        "state": state,
    }))
}

pub(super) fn diagnostic_invocation_to_json(diagnostic: &Diagnostic) -> Value {
    fn to_json_node(node_arc: Arc<Mutex<TreeNode<UInt160>>>) -> Value {
        let (hash, children) = {
            let node = node_arc.lock();
            (node.item().to_string(), node.children().to_vec())
        };
        let mut obj = Map::new();
        obj.insert("hash".to_string(), Value::String(hash));
        if !children.is_empty() {
            let children = children.into_iter().map(to_json_node).collect::<Vec<_>>();
            obj.insert("call".to_string(), Value::Array(children));
        }
        Value::Object(obj)
    }

    match diagnostic.root() {
        Some(root) => to_json_node(root),
        None => Value::Null,
    }
}

pub(super) fn diagnostic_storage_changes(engine: &ApplicationEngine) -> Value {
    let changes = engine.snapshot_cache().tracked_items();
    let entries = changes
        .into_iter()
        .map(|(key, trackable)| {
            json!({
                "state": format!("{:?}", trackable.state),
                "key": BASE64_STANDARD.encode(key.to_array()),
                "value": BASE64_STANDARD.encode(trackable.item.get_value()),
            })
        })
        .collect::<Vec<_>>();
    Value::Array(entries)
}

pub(super) fn expect_string_param(
    params: &[Value],
    index: usize,
    method: &str,
) -> Result<String, RpcException> {
    params
        .get(index)
        .and_then(|value| value.as_str())
        .map(std::string::ToString::to_string)
        .ok_or_else(|| {
            RpcException::from(RpcError::invalid_params().with_data(format!(
                "{} expects string parameter {}",
                method,
                index + 1
            )))
        })
}

pub(super) fn expect_u32_param(
    params: &[Value],
    index: usize,
    method: &str,
) -> Result<u32, RpcException> {
    let value = params.get(index).ok_or_else(|| {
        RpcException::from(RpcError::invalid_params().with_data(format!(
            "{} expects integer parameter {}",
            method,
            index + 1
        )))
    })?;
    if let Some(number) = value.as_u64() {
        if u32::try_from(number).is_ok() {
            return Ok(number as u32);
        }
    }
    Err(RpcException::from(RpcError::invalid_params().with_data(
        format!("{} expects integer parameter {}", method, index + 1),
    )))
}

pub(super) fn expect_uuid_param(
    params: &[Value],
    index: usize,
    method: &str,
) -> Result<Uuid, RpcException> {
    let text = expect_string_param(params, index, method)?;
    Uuid::parse_str(text.trim()).map_err(|_| {
        RpcException::from(RpcError::invalid_params().with_data(format!(
            "{} expects GUID parameter {}",
            method,
            index + 1
        )))
    })
}

pub(super) fn invalid_params(message: impl Into<String>) -> RpcException {
    RpcException::from(RpcError::invalid_params().with_data(message.into()))
}

pub(super) fn internal_error(message: impl Into<String>) -> RpcException {
    RpcException::from(RpcError::internal_server_error().with_data(message.into()))
}
