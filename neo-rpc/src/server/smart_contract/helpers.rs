use neo_execution::contract_parameter::ContractParameter;
use neo_payloads::NotifyEventArgs;
use neo_payloads::signer::Signer;
use neo_payloads::witness::Witness;
use neo_primitives::UInt160;
use neo_serialization::json::JToken;
use neo_vm::rpc_json::StackItemRpcJson;
use neo_vm::stack_item::StackItem;
use neo_vm_rs::VmState;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::server::model::signers_and_witnesses::SignersAndWitnesses;
use crate::server::parameter_converter::{ConversionContext, ParameterConverter};
use crate::server::rpc_exception::RpcException;
pub(super) use crate::server::rpc_helpers::{
    expect_string_param, expect_u32_param, expect_uint160_param_with_message, internal_error,
    invalid_params,
};
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
            .map(|value| {
                ContractParameter::from_json(value).map_err(|e| invalid_params(e.to_string()))
            })
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

pub(super) fn expect_script_hash_param(
    params: &[Value],
    index: usize,
    method: &str,
) -> Result<UInt160, RpcException> {
    expect_uint160_param_with_message(
        params,
        index,
        format!("{} expects string parameter {}", method, index + 1),
        "script hash",
    )
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
    for entry in &notification.state {
        state.push(stack_item_to_json(entry, session.as_deref_mut())?);
    }
    Ok(json!({
        "eventname": notification.event_name,
        "contract": notification.script_hash.to_string(),
        "state": state}))
}

pub(super) fn expect_uuid_param(
    params: &[Value],
    index: usize,
    method: &str,
) -> Result<Uuid, RpcException> {
    let text = expect_string_param(params, index, method)?;
    Uuid::parse_str(text.trim())
        .map_err(|_| invalid_params(format!("{} expects GUID parameter {}", method, index + 1)))
}
