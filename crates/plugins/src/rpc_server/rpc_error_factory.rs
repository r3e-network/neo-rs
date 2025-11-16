// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of Neo.Plugins.RpcServer.RpcErrorFactory providing helper
// constructors for specialised `RpcError` instances.

use neo_core::cryptography::crypto_utils::ECPoint;
use neo_core::UInt160;

use super::rpc_error::RpcError;

pub fn with_data(error: &RpcError, data: impl Into<String>) -> RpcError {
    error.with_data(data)
}

pub fn new_custom_error(code: i32, message: impl Into<String>, data: Option<String>) -> RpcError {
    RpcError::new(code, message, data)
}

pub fn already_exists(data: impl Into<String>) -> RpcError {
    RpcError::already_exists().with_data(data)
}

pub fn invalid_params(data: impl Into<String>) -> RpcError {
    RpcError::invalid_params().with_data(data)
}

pub fn bad_request(data: impl Into<String>) -> RpcError {
    RpcError::bad_request().with_data(data)
}

pub fn invalid_contract_verification_hash(contract_hash: &UInt160, pcount: i32) -> RpcError {
    RpcError::invalid_contract_verification().with_data(format!(
        "The smart contract {} haven't got verify method with {} input parameters.",
        contract_hash, pcount
    ))
}

pub fn invalid_contract_verification(data: impl Into<String>) -> RpcError {
    RpcError::invalid_contract_verification().with_data(data)
}

pub fn invalid_signature(data: impl Into<String>) -> RpcError {
    RpcError::invalid_signature().with_data(data)
}

pub fn oracle_not_designated_node(oracle_pub: &ECPoint) -> RpcError {
    RpcError::oracle_not_designated_node()
        .with_data(format!("{:?} isn't an oracle node.", oracle_pub))
}
