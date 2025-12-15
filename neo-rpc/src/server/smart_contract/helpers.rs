use std::collections::BTreeMap;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::contract_parameter::{ContractParameter, ContractParameterValue};
use neo_core::smart_contract::notify_event_args::NotifyEventArgs;
use neo_core::smart_contract::ApplicationEngine;
use neo_core::UInt160;
use neo_json::JToken;
use parking_lot::Mutex;
use serde_json::{json, Map, Number as JsonNumber, Value};
use uuid::Uuid;

use crate::server::diagnostic::Diagnostic;
use crate::server::model::signers_and_witnesses::SignersAndWitnesses;
use crate::server::parameter_converter::{ConversionContext, ParameterConverter};
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;
use crate::server::session::Session;
use crate::server::tree_node::TreeNode;

use neo_vm::stack_item::StackItem;

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

    builder.emit_push_int(CallFlags::ALL.bits() as i64);
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
        ContractParameterValue::Hash160(value) => {
            Ok(StackItem::from_byte_string(value.to_bytes().to_vec()))
        }
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
            let mut map = BTreeMap::new();
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
    mut session: Option<&mut Session>,
) -> Result<Value, RpcException> {
    let mut obj = Map::new();
    match item {
        StackItem::Null => {
            obj.insert("type".to_string(), Value::String("Any".to_string()));
            Ok(Value::Object(obj))
        }
        StackItem::Boolean(value) => {
            obj.insert("type".to_string(), Value::String("Boolean".to_string()));
            obj.insert("value".to_string(), Value::Bool(*value));
            Ok(Value::Object(obj))
        }
        StackItem::Integer(value) => {
            obj.insert("type".to_string(), Value::String("Integer".to_string()));
            obj.insert("value".to_string(), Value::String(value.to_string()));
            Ok(Value::Object(obj))
        }
        StackItem::ByteString(bytes) => {
            obj.insert("type".to_string(), Value::String("ByteString".to_string()));
            obj.insert(
                "value".to_string(),
                Value::String(BASE64_STANDARD.encode(bytes)),
            );
            Ok(Value::Object(obj))
        }
        StackItem::Buffer(buffer) => {
            obj.insert("type".to_string(), Value::String("Buffer".to_string()));
            obj.insert(
                "value".to_string(),
                Value::String(BASE64_STANDARD.encode(buffer.data())),
            );
            Ok(Value::Object(obj))
        }
        StackItem::Array(array) => {
            obj.insert("type".to_string(), Value::String("Array".to_string()));
            let values = array
                .items()
                .iter()
                .map(|entry| stack_item_to_json(entry, session.as_deref_mut()))
                .collect::<Result<Vec<_>, _>>()?;
            obj.insert("value".to_string(), Value::Array(values));
            Ok(Value::Object(obj))
        }
        StackItem::Struct(items) => {
            obj.insert("type".to_string(), Value::String("Struct".to_string()));
            let values = items
                .items()
                .iter()
                .map(|entry| stack_item_to_json(entry, session.as_deref_mut()))
                .collect::<Result<Vec<_>, _>>()?;
            obj.insert("value".to_string(), Value::Array(values));
            Ok(Value::Object(obj))
        }
        StackItem::Map(map) => {
            obj.insert("type".to_string(), Value::String("Map".to_string()));
            let entries = map
                .iter()
                .map(|(key, value)| {
                    let key_json = stack_item_to_json(key, session.as_deref_mut())?;
                    let value_json = stack_item_to_json(value, session.as_deref_mut())?;
                    Ok(json!({
                        "key": key_json,
                        "value": value_json,
                    }))
                })
                .collect::<Result<Vec<_>, RpcException>>()?;
            obj.insert("value".to_string(), Value::Array(entries));
            Ok(Value::Object(obj))
        }
        StackItem::Pointer(pointer) => {
            obj.insert("type".to_string(), Value::String("Pointer".to_string()));
            obj.insert(
                "value".to_string(),
                Value::Number(JsonNumber::from(pointer.position() as u64)),
            );
            Ok(Value::Object(obj))
        }
        StackItem::InteropInterface(iface) => {
            obj.insert(
                "type".to_string(),
                Value::String("InteropInterface".to_string()),
            );
            let mut value_obj = Map::new();
            value_obj.insert(
                "type".to_string(),
                Value::String(iface.interface_type().to_string()),
            );
            obj.insert("value".to_string(), Value::Object(value_obj));
            if let Some(session) = session.as_mut() {
                if let Some(iterator_id) = session.register_iterator_interface(iface) {
                    obj.insert(
                        "interface".to_string(),
                        Value::String("IIterator".to_string()),
                    );
                    obj.insert("id".to_string(), Value::String(iterator_id.to_string()));
                }
            }
            Ok(Value::Object(obj))
        }
    }
}

pub(super) fn notification_to_json(
    notification: &NotifyEventArgs,
    mut session: Option<&mut Session>,
) -> Result<Value, RpcException> {
    let mut state = Vec::new();
    for entry in notification.state.iter() {
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
        .map(|value| value.to_string())
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
        if number <= u32::MAX as u64 {
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
