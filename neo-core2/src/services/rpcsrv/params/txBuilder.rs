use std::error::Error;
use std::fmt;

use crate::crypto::keys;
use crate::io;
use crate::smartcontract;
use crate::smartcontract::callflag;
use crate::util;
use crate::vm::emit;
use crate::vm::opcode;

#[derive(Debug)]
pub struct UnsupportedError;

impl fmt::Display for UnsupportedError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "unsupported parameter type")
    }
}

impl Error for UnsupportedError {}

pub fn expand_func_parameter_into_script(script: &mut io::BinWriter, fp: FuncParam) -> Result<(), Box<dyn Error>> {
    match fp.param_type {
        smartcontract::ParamType::ByteArray => {
            let str = fp.value.get_bytes_base64()?;
            emit::bytes(script, &str);
        }
        smartcontract::ParamType::Signature => {
            let str = fp.value.get_bytes_base64()?;
            emit::bytes(script, &str);
        }
        smartcontract::ParamType::String => {
            let str = fp.value.get_string()?;
            emit::string(script, &str);
        }
        smartcontract::ParamType::Hash160 => {
            let hash = fp.value.get_uint160_from_hex()?;
            emit::bytes(script, &hash.bytes_be());
        }
        smartcontract::ParamType::Hash256 => {
            let hash = fp.value.get_uint256()?;
            emit::bytes(script, &hash.bytes_be());
        }
        smartcontract::ParamType::PublicKey => {
            let str = fp.value.get_string()?;
            let key = keys::PublicKey::from_string(&str)?;
            emit::bytes(script, &key.bytes());
        }
        smartcontract::ParamType::Integer => {
            let bi = fp.value.get_big_int()?;
            emit::big_int(script, &bi);
        }
        smartcontract::ParamType::Bool => {
            let val = fp.value.get_boolean()?;
            emit::bool(script, val);
        }
        smartcontract::ParamType::Array => {
            let val = fp.value.get_array()?;
            expand_array_into_script_and_pack(script, &val)?;
        }
        smartcontract::ParamType::Map => {
            let val = fp.value.get_array()?;
            expand_map_into_script_and_pack(script, &val)?;
        }
        smartcontract::ParamType::Any => {
            if fp.value.is_null() || fp.value.raw_message.is_empty() {
                emit::opcodes(script, &[opcode::PUSHNULL]);
            }
        }
        _ => return Err(Box::new(UnsupportedError)),
    }
    Ok(())
}

pub fn expand_array_into_script(script: &mut io::BinWriter, slice: &[Param]) -> Result<(), Box<dyn Error>> {
    for j in (0..slice.len()).rev() {
        let fp = slice[j].get_func_param()?;
        expand_func_parameter_into_script(script, fp)?;
    }
    Ok(())
}

pub fn expand_array_into_script_and_pack(script: &mut io::BinWriter, slice: &[Param]) -> Result<(), Box<dyn Error>> {
    if slice.is_empty() {
        emit::opcodes(script, &[opcode::NEWARRAY0]);
        return Ok(());
    }
    expand_array_into_script(script, slice)?;
    emit::int(script, slice.len() as i64);
    emit::opcodes(script, &[opcode::PACK]);
    Ok(())
}

pub fn expand_map_into_script_and_pack(script: &mut io::BinWriter, slice: &[Param]) -> Result<(), Box<dyn Error>> {
    if slice.is_empty() {
        emit::opcodes(script, &[opcode::NEWMAP]);
        return Ok(());
    }
    for i in (0..slice.len()).rev() {
        let pair = slice[i].get_func_param_pair()?;
        expand_func_parameter_into_script(script, pair.value)?;
        expand_func_parameter_into_script(script, pair.key)?;
    }
    emit::int(script, slice.len() as i64);
    emit::opcodes(script, &[opcode::PACKMAP]);
    Ok(())
}

pub fn create_function_invocation_script(contract: util::Uint160, method: &str, param: Option<&Param>) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut script = io::BufBinWriter::new();
    if let Some(param) = param {
        if let Ok(slice) = param.get_array() {
            expand_array_into_script_and_pack(&mut script, &slice)?;
        } else {
            return Err(Box::new(fmt::Error));
        }
    } else {
        emit::opcodes(&mut script, &[opcode::NEWARRAY0]);
    }

    emit::app_call_no_args(&mut script, &contract, method, callflag::ALL);
    Ok(script.bytes())
}
