//! Shared address and hash text parsers for RPC parameter conversion.

use neo_primitives::UInt160;

use super::super::model::Address;
use super::super::rpc_exception::RpcException;
use super::errors::invalid_params;

pub(super) fn parse_address(text: &str, address_version: u8) -> Result<Address, RpcException> {
    crate::protocol::address::parse_script_hash_or_address(text, address_version)
        .map(|hash| Address::new(hash, address_version))
        .map_err(|_| invalid_params(format!("Invalid address: {text}")))
}

pub(super) fn parse_uint160(text: &str) -> Result<UInt160, RpcException> {
    let mut result = None;
    if UInt160::try_parse(text, &mut result) {
        if let Some(value) = result {
            return Ok(value);
        }
    }
    Err(invalid_params(format!("Invalid UInt160 value: {text}")))
}
