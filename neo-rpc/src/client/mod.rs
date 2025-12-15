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

mod contract_client;
mod error;
pub mod models;
mod nep17_api;
mod policy_api;
mod rpc_client;
mod state_api;
mod transaction_manager;
mod transaction_manager_factory;
mod utility;
mod wallet_api;

pub use contract_client::ContractClient;
pub use error::{ClientRpcError, RpcException};
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
