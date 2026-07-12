//! Dynamic-call script construction for smart-contract RPC handlers.
//!
//! `invokefunction` and `invokecontractverify` both need Neo contract
//! parameters converted into VM stack values. This module owns that bytecode
//! and conversion boundary so generic helper code does not mix RPC parsing with
//! script layout.

use neo_execution::contract_parameter::{ContractParameter, ContractParameterValue};
use neo_manifest::CallFlags;
use neo_primitives::UInt160;
use neo_vm_rs::StackValue;
use num_traits::ToPrimitive;

use crate::server::rpc_exception::RpcException;

use super::helpers::{internal_error, invalid_params};

pub(super) fn build_dynamic_call_script(
    script_hash: UInt160,
    operation: &str,
    parameters: &[ContractParameter],
) -> Result<Vec<u8>, RpcException> {
    let args = parameters
        .iter()
        .map(contract_parameter_to_stack_value)
        .collect::<Result<Vec<_>, _>>()?;
    let mut builder = neo_vm::script_builder::ScriptBuilder::new();

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

pub(in crate::server::smart_contract) fn contract_parameter_to_stack_value(
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
