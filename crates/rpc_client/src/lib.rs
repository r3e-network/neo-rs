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
mod state_api;
mod utility;

#[cfg(feature = "full")]
mod contract_client;
#[cfg(feature = "full")]
mod nep17_api;
#[cfg(feature = "full")]
mod policy_api;
#[cfg(feature = "full")]
mod transaction_manager;
#[cfg(feature = "full")]
mod transaction_manager_factory;
#[cfg(feature = "full")]
mod wallet_api;

#[cfg(feature = "full")]
pub use contract_client::ContractClient;
#[cfg(feature = "full")]
pub use nep17_api::Nep17Api;
#[cfg(feature = "full")]
pub use policy_api::PolicyApi;
pub use rpc_client::RpcClient;
pub use rpc_exception::RpcException;
pub use state_api::StateApi;
#[cfg(feature = "full")]
pub use transaction_manager::TransactionManager;
#[cfg(feature = "full")]
pub use transaction_manager_factory::TransactionManagerFactory;
pub use utility::Utility;
#[cfg(feature = "full")]
pub use wallet_api::WalletApi;

// Re-export commonly used types
pub use models::{
    RpcBlock, RpcBlockHeader, RpcRequest, RpcResponse, RpcResponseError, RpcTransaction,
};
