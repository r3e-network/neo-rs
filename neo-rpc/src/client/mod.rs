//! # neo-rpc::client
//!
//! Client-side adapters for remote services and RPC access.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `contract_client`: contract RPC client facade.
//! - `contract_script`: contract script invocation helpers.
//! - `error`: Typed error definitions and conversions.
//! - `models`: RPC request and response models.
//! - `nep17_api`: NEP-17 RPC client helpers.
//! - `policy_api`: policy RPC client helpers.
//! - `rpc_client`: HTTP JSON-RPC client implementation.
//! - `state_api`: state RPC client helpers.
//! - `test_helpers`: RPC client test helpers.
//! - `transaction_manager`: transaction submission helpers.
//! - `transaction_manager_factory`: transaction manager factory types and
//!   helpers.
//! - `utility`: utility RPC client helpers.
//! - `wallet_api`: wallet RPC client helpers.

#[path = "contracts/contract_client.rs"]
mod contract_client;
#[path = "contracts/contract_script.rs"]
mod contract_script;
#[path = "errors/error.rs"]
mod error;
pub mod models;
#[path = "apis/nep17_api.rs"]
mod nep17_api;
#[path = "apis/policy_api.rs"]
mod policy_api;
mod rpc_client;
#[path = "apis/state_api.rs"]
mod state_api;
#[cfg(test)]
#[path = "../tests/client/test_helpers.rs"]
mod test_helpers;
#[path = "transactions/transaction_manager.rs"]
mod transaction_manager;
#[path = "transactions/transaction_manager_factory.rs"]
mod transaction_manager_factory;
mod utility;
#[path = "apis/wallet_api.rs"]
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
#[cfg(feature = "server")]
pub(crate) use utility::parse_script_hash_or_address_inner;
pub use wallet_api::WalletApi;

// Re-export commonly used types
pub use models::{
    RpcBlock, RpcBlockHeader, RpcRequest, RpcResponse, RpcResponseError, RpcTransaction,
};
