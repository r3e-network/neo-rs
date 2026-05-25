use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::contract_parameter::{ContractParameter, ContractParameterValue};
use neo_core::smart_contract::notify_event_args::NotifyEventArgs;
use neo_core::smart_contract::ApplicationEngine;
use neo_core::vm_runtime::rpc_json::stack_item_rpc_json_deferred_size_check;
use neo_core::vm_runtime::StackItem;
use neo_core::UInt160;
use neo_json::JToken;
use neo_vm_rs::{StackValue, VmState};
use num_traits::ToPrimitive;
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::server::diagnostic::{Diagnostic, DiagnosticInvocation};
use crate::server::model::signers_and_witnesses::SignersAndWitnesses;
use crate::server::parameter_converter::{ConversionContext, ParameterConverter};
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;
use crate::server::session::Session;

const INVALID_OPERATION_CODE: i32 = -2146233079;

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

pub(super) fn final_rpc_vm_state_string(state: VmState) -> Result<String, RpcException> {
    state
        .final_name()
        .map(str::to_string)
        .ok_or_else(|| internal_error(format!("{state:?} is not a final VM state")))
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
        .map(contract_parameter_to_stack_value)
        .collect::<Result<Vec<_>, _>>()?;
    let mut builder = neo_core::script_builder::ScriptBuilder::new();

    if args.is_empty() {
        builder.emit_opcode(neo_vm_rs::OpCode::NEWARRAY0);
    } else {
        for item in args.iter().rev() {
            builder
                .emit_push_stack_value(item)
                .map_err(|err| internal_error(err.to_string()))?;
        }
        builder.emit_push_int(args.len() as i64);
        builder.emit_opcode(neo_vm_rs::OpCode::PACK);
    }

    builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    builder.emit_push(operation.as_bytes());
    builder.emit_push(script_hash.to_bytes().as_ref());
    builder
        .emit_syscall("System.Contract.Call")
        .map_err(|err| internal_error(err.to_string()))?;

    Ok(builder.to_array())
}

pub(super) fn contract_parameter_to_stack_value(
    parameter: &ContractParameter,
) -> Result<StackValue, RpcException> {
    match &parameter.value {
        ContractParameterValue::Any | ContractParameterValue::Void => Ok(StackValue::Null),
        ContractParameterValue::Boolean(value) => Ok(StackValue::Boolean(*value)),
        ContractParameterValue::Integer(value) => Ok(if let Some(value) = value.to_i64() {
            StackValue::Integer(value)
        } else {
            StackValue::BigInteger(value.to_signed_bytes_le())
        }),
        ContractParameterValue::Hash160(value) => Ok(StackValue::ByteString(value.to_bytes())),
        ContractParameterValue::Hash256(value) => {
            Ok(StackValue::ByteString(value.to_array().to_vec()))
        }
        ContractParameterValue::ByteArray(bytes) | ContractParameterValue::Signature(bytes) => {
            Ok(StackValue::ByteString(bytes.clone()))
        }
        ContractParameterValue::PublicKey(point) => Ok(StackValue::ByteString(point.encoded())),
        ContractParameterValue::String(value) => {
            Ok(StackValue::ByteString(value.as_bytes().to_vec()))
        }
        ContractParameterValue::Array(items) => {
            let converted = items
                .iter()
                .map(contract_parameter_to_stack_value)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(StackValue::Array(converted))
        }
        ContractParameterValue::Map(entries) => {
            let mut map = Vec::with_capacity(entries.len());
            for (key, value) in entries {
                map.push((
                    contract_parameter_to_stack_value(key)?,
                    contract_parameter_to_stack_value(value)?,
                ));
            }
            Ok(StackValue::Map(map))
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
    let mut value = stack_item_rpc_json_deferred_size_check(item, max_size)
        .map_err(|err| stack_item_error(err.to_string()))?;
    if let StackItem::InteropInterface(iface) = item {
        if let Some(session) = session {
            if let Some(iterator_id) = session.register_iterator_interface(iface) {
                if let Value::Object(obj) = &mut value {
                    obj.insert(
                        "interface".to_string(),
                        Value::String("StorageIterator".to_string()),
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
    fn to_json_node(node: DiagnosticInvocation) -> Value {
        let mut obj = Map::new();
        obj.insert("hash".to_string(), Value::String(node.hash.to_string()));
        if !node.children.is_empty() {
            let children = node
                .children
                .into_iter()
                .map(to_json_node)
                .collect::<Vec<_>>();
            obj.insert("call".to_string(), Value::Array(children));
        }
        Value::Object(obj)
    }

    match diagnostic.invocation_root() {
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
                "value": BASE64_STANDARD.encode(&*trackable.item.value_bytes()),
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
