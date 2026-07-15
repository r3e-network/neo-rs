//! Dynamic-call script construction for smart-contract RPC handlers.
//!
//! `invokefunction` and `invokecontractverify` both need Neo contract
//! parameters emitted into VM bytecode. This module owns that boundary so
//! generic helper code does not mix RPC parsing with script layout.

use neo_execution::contract_parameter::{ContractParameter, ContractParameterValue};
use neo_manifest::CallFlags;
use neo_primitives::UInt160;
use neo_vm::OpCode;
use neo_vm::script_builder::ScriptBuilder;

use crate::server::rpc_exception::RpcException;

use super::helpers::{internal_error, invalid_params};

pub(super) fn build_dynamic_call_script(
    script_hash: UInt160,
    operation: &str,
    parameters: &[ContractParameter],
) -> Result<Vec<u8>, RpcException> {
    let mut builder = ScriptBuilder::new();

    if parameters.is_empty() {
        builder.emit_opcode(OpCode::NEWARRAY0);
    } else {
        for parameter in parameters.iter().rev() {
            emit_contract_parameter(&mut builder, parameter)?;
        }
        builder.emit_push_int(parameters.len() as i64);
        builder.emit_opcode(OpCode::PACK);
    }

    builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    builder.emit_push(operation.as_bytes());
    builder.emit_push(script_hash.to_bytes().as_ref());
    builder
        .emit_syscall("System.Contract.Call")
        .map_err(|err| internal_error(err.to_string()))?;

    Ok(builder.to_array())
}

pub(in crate::server::smart_contract) fn emit_contract_parameter(
    builder: &mut ScriptBuilder,
    parameter: &ContractParameter,
) -> Result<(), RpcException> {
    match &parameter.value {
        ContractParameterValue::Any | ContractParameterValue::Void => {
            builder.emit_opcode(OpCode::PUSHNULL);
        }
        ContractParameterValue::Boolean(value) => {
            builder.emit_push_bool(*value);
        }
        ContractParameterValue::Integer(value) => {
            builder
                .emit_push_bigint(value.clone())
                .map_err(|error| internal_error(error.to_string()))?;
        }
        ContractParameterValue::Hash160(value) => {
            builder.emit_push(&value.to_bytes());
        }
        ContractParameterValue::Hash256(value) => {
            builder.emit_push(&value.to_array());
        }
        ContractParameterValue::ByteArray(bytes) | ContractParameterValue::Signature(bytes) => {
            builder.emit_push(bytes);
        }
        ContractParameterValue::PublicKey(point) => {
            builder.emit_push(&point.encoded());
        }
        ContractParameterValue::String(value) => {
            builder.emit_push(value.as_bytes());
        }
        ContractParameterValue::Array(items) => {
            for item in items.iter().rev() {
                emit_contract_parameter(builder, item)?;
            }
            builder.emit_push_int(items.len() as i64);
            builder.emit_opcode(OpCode::PACK);
        }
        ContractParameterValue::Map(entries) => {
            if entries.is_empty() {
                builder.emit_opcode(OpCode::NEWMAP);
            } else {
                for (key, value) in entries.iter().rev() {
                    emit_contract_parameter(builder, value)?;
                    emit_contract_parameter(builder, key)?;
                }
                builder.emit_push_int(entries.len() as i64);
                builder.emit_opcode(OpCode::PACKMAP);
            }
        }
        ContractParameterValue::InteropInterface => {
            return Err(invalid_params(
                "InteropInterface parameters are not supported in invoke RPCs",
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::ContractParameterType;
    use num_bigint::BigInt;

    fn integer(value: i64) -> ContractParameter {
        ContractParameter::with_value(
            ContractParameterType::Integer,
            ContractParameterValue::Integer(BigInt::from(value)),
        )
    }

    #[test]
    fn map_parameters_use_packmap_without_deduplicating_entries() {
        let parameter = ContractParameter::with_value(
            ContractParameterType::Map,
            ContractParameterValue::Map(vec![(integer(1), integer(2)), (integer(1), integer(3))]),
        );
        let mut builder = ScriptBuilder::new();

        emit_contract_parameter(&mut builder, &parameter).expect("emit map");

        assert_eq!(
            builder.to_array(),
            vec![
                OpCode::PUSH3.byte(),
                OpCode::PUSH1.byte(),
                OpCode::PUSH2.byte(),
                OpCode::PUSH1.byte(),
                OpCode::PUSH2.byte(),
                OpCode::PACKMAP.byte(),
            ]
        );
    }

    #[test]
    fn empty_map_parameter_uses_newmap() {
        let parameter = ContractParameter::with_value(
            ContractParameterType::Map,
            ContractParameterValue::Map(Vec::new()),
        );
        let mut builder = ScriptBuilder::new();

        emit_contract_parameter(&mut builder, &parameter).expect("emit empty map");

        assert_eq!(builder.to_array(), vec![OpCode::NEWMAP.byte()]);
    }
}
