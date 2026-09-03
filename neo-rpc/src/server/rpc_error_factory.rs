// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of Neo.Plugins.RpcServer.RpcErrorFactory providing helper
// constructors for specialised `RpcError` instances.

use neo_core::UInt160;

use super::rpc_error::RpcError;

pub fn invalid_contract_verification_hash(contract_hash: &UInt160, pcount: i32) -> RpcError {
    RpcError::invalid_contract_verification().with_data(format!(
        "The smart contract {contract_hash} haven't got verify method with {pcount} input parameters."
    ))
}

pub fn invalid_contract_verification(data: impl Into<String>) -> RpcError {
    RpcError::invalid_contract_verification().with_data(data)
}
