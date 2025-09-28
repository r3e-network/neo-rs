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

pub mod models;
mod rpc_client;
mod rpc_exception;
mod utility;
mod contract_client;
mod nep17_api;
mod policy_api;
mod state_api;
mod wallet_api;
mod transaction_manager;
mod transaction_manager_factory;

pub use rpc_client::RpcClient;
pub use rpc_exception::RpcException;
pub use utility::Utility;
pub use contract_client::ContractClient;
pub use nep17_api::Nep17Api;
pub use policy_api::PolicyApi;
pub use state_api::StateApi;
pub use wallet_api::WalletApi;
pub use transaction_manager::TransactionManager;
pub use transaction_manager_factory::TransactionManagerFactory;

// Re-export commonly used types
pub use models::{
    RpcRequest, RpcResponse, RpcResponseError,
    RpcBlock, RpcBlockHeader, RpcTransaction,
};
