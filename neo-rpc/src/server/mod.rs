//! RPC Server implementation
//!
//! This module provides the JSON-RPC server for Neo N3 nodes.
//! Previously implemented as a plugin, now integrated directly into neo-rpc.

mod diagnostic;
pub(crate) mod dispatch;
pub(crate) mod jsonrpsee_adapter;
mod ledger_queries;
pub mod middleware;
pub mod model;
mod native_queries;
mod parameter_converter;
mod rpc_error;
mod rpc_error_factory;
mod rpc_exception;
mod rpc_handler_macros;
pub mod rpc_helpers;
mod rpc_method_attribute;
mod rpc_registry;
mod rpc_relay;
mod rpc_server;
mod rpc_server_application_logs;
mod rpc_server_blockchain;
mod rpc_server_indexer;
mod rpc_server_node;
mod rpc_server_oracle;
mod rpc_server_settings;
mod rpc_server_state;
mod rpc_server_tokens_tracker;
mod rpc_server_utilities;
mod rpc_server_wallet;
mod rpc_tls;
mod rpc_transport;
mod session;
pub mod smart_contract;
#[cfg(test)]
pub(crate) mod test_support;
mod wallet_compat;
pub mod ws;

// Public exports
pub use rpc_error::RpcError as ServerRpcError;
pub use rpc_exception::RpcException;
pub(crate) use rpc_handler_macros::rpc_handlers;
pub use rpc_method_attribute::RpcMethodDescriptor;
pub use rpc_registry::{SERVERS, ServerRegistry};
pub use rpc_server::{RpcCallback, RpcHandler, RpcServer};
pub use rpc_server_application_logs::RpcServerApplicationLogs;
pub use rpc_server_blockchain::RpcServerBlockchain;
pub use rpc_server_indexer::RpcServerIndexer;
pub use rpc_server_node::RpcServerNode;
pub use rpc_server_oracle::RpcServerOracle;
pub use rpc_server_settings::{RpcServerConfig, RpcServerSettings};
pub use rpc_server_state::RpcServerState;
pub use rpc_server_tokens_tracker::RpcServerTokensTracker;
pub use rpc_server_utilities::RpcServerUtilities;
pub use rpc_server_wallet::RpcServerWallet;
pub use rpc_tls::build_tls_config_from_settings;
pub use session::Session;

#[cfg(feature = "server")]
pub use jsonrpsee_adapter::{
    JsonRpseeContext, build_jsonrpsee_module, build_jsonrpsee_module_with_disabled,
};

// Re-export smart contract handlers
pub use smart_contract::RpcServerSmartContract;

// Re-export WebSocket types
pub use ws::{SharedWsEventBridge, WsEvent, WsEventBridge, WsEventType, WsNotification};
