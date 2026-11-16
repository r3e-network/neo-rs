// Copyright (C) 2015-2025 The Neo Project.
//
// lib.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Neo RPC Client Library
//!
//! This crate provides a complete RPC client implementation for interacting with Neo nodes.
//! It matches the C# RpcClient implementation exactly.
//!
//! Phase 2 S5: extracted from `neo-rpc::client` into a standalone crate. It
//! also owns the shared RPC vocabulary (`error`, `error_code`, `serialization`)
//! consumed by both the client and `neo-rpc-server`. The historical
//! `neo_rpc::client::*` paths remain available through the `neo-rpc` facade.

mod client_error;
mod contract_client;
mod contract_script;
pub mod error;
pub mod error_code;
pub mod models;
mod nep17_api;
mod policy_api;
mod rpc_client;
pub mod serialization;
mod state_api;
#[cfg(test)]
mod test_helpers;
mod transaction_manager;
mod transaction_manager_factory;
mod utility;
mod wallet_api;

pub use client_error::{ClientRpcError, RpcException};
pub use contract_client::ContractClient;
pub use error::{RpcError, RpcResult};
pub use error_code::RpcErrorCode;
pub use nep17_api::Nep17Api;
pub use policy_api::PolicyApi;
pub use rpc_client::{RpcClient, RpcClientBuilder, RpcClientHooks, RpcRequestOutcome};
pub use state_api::StateApi;
pub use transaction_manager::TransactionManager;
pub use transaction_manager_factory::TransactionManagerFactory;
pub use utility::RpcUtility;
pub use wallet_api::WalletApi;

// Re-export commonly used types
pub use models::{
    RpcBlock, RpcBlockHeader, RpcRequest, RpcResponse, RpcResponseError, RpcTransaction,
};
