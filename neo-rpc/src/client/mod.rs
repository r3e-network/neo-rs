//! Neo RPC Client Library
//!
//! This crate provides a complete RPC client implementation for interacting with Neo nodes.
//! It matches the C# RpcClient implementation exactly.

mod contract_client;
mod contract_script;
mod error;
pub mod models;
mod nep17_api;
mod policy_api;
mod rpc_client;
mod state_api;
#[cfg(test)]
mod test_helpers;
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
