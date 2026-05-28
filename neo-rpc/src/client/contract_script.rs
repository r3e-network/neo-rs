use crate::RpcError;
use neo_core::ScriptBuilder;
use neo_core::smart_contract::CallFlags;
use neo_primitives::UInt160;
use neo_vm_rs::OpCode;

pub(crate) fn build_dynamic_call_script(
    script_hash: &UInt160,
    method: &str,
    args: &[serde_json::Value],
    call_flags: CallFlags,
) -> Result<Vec<u8>, RpcError> {
    let mut sb = ScriptBuilder::new();

    emit_json_args_array(&mut sb, args)?;
    emit_contract_call(&mut sb, script_hash, method, call_flags)?;

    Ok(sb.to_array())
}

pub(crate) fn emit_json_args_array(
    sb: &mut ScriptBuilder,
    args: &[serde_json::Value],
) -> Result<(), RpcError> {
    if args.is_empty() {
        sb.emit_opcode(OpCode::NEWARRAY0);
    } else {
        for arg in args.iter().rev() {
            emit_json_argument(sb, arg)?;
        }
        sb.emit_push_int(args.len() as i64);
        sb.emit_pack();
    }

    Ok(())
}

pub(crate) fn emit_json_argument(
    sb: &mut ScriptBuilder,
    arg: &serde_json::Value,
) -> Result<(), RpcError> {
    match arg {
        serde_json::Value::Null => {
            sb.emit_opcode(OpCode::PUSHNULL);
            Ok(())
        }
        serde_json::Value::Bool(value) => {
            sb.emit_push_bool(*value);
            Ok(())
        }
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                sb.emit_push_int(value);
                Ok(())
            } else if let Some(value) = value.as_u64() {
                sb.emit_push_int(value as i64);
                Ok(())
            } else {
                Err("Invalid number format".into())
            }
        }
        serde_json::Value::String(value) => {
            sb.emit_push(value.as_bytes());
            Ok(())
        }
        serde_json::Value::Array(values) => {
            for value in values.iter().rev() {
                emit_json_argument(sb, value)?;
            }
            sb.emit_push_int(values.len() as i64);
            sb.emit_pack();
            Ok(())
        }
        _ => Err("Unsupported argument type".into()),
    }
}

pub(crate) fn emit_contract_call(
    sb: &mut ScriptBuilder,
    script_hash: &UInt160,
    method: &str,
    call_flags: CallFlags,
) -> Result<(), RpcError> {
    sb.emit_push_int(i64::from(call_flags.bits()));
    sb.emit_push(method.as_bytes());
    sb.emit_push(&script_hash.to_array());
    sb.emit_syscall("System.Contract.Call")
        .map_err(|err| RpcError::invalid_params(err.to_string()))?;
    Ok(())
}
