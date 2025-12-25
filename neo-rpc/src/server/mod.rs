//! RPC Server implementation
//!
//! This module provides the JSON-RPC server for Neo N3 nodes.
//! Previously implemented as a plugin, now integrated directly into neo-rpc.

mod diagnostic;
pub mod middleware;
pub mod model;
mod parameter_converter;
mod rcp_server_settings;
mod routes;
mod rpc_error;
mod rpc_error_factory;
mod rpc_exception;
mod rpc_method_attribute;
mod rpc_server;
mod rpc_server_blockchain;
mod rpc_server_node;
mod rpc_server_state;
mod rpc_server_utilities;
mod rpc_server_wallet;
mod session;
pub mod smart_contract;
mod tree;
mod tree_node;
pub mod ws;

// Public exports
pub use rcp_server_settings::{RpcServerConfig, RpcServerSettings};
pub use rpc_error::RpcError as ServerRpcError;
pub use rpc_exception::RpcException;
pub use rpc_method_attribute::RpcMethodDescriptor;
pub use rpc_server::{
    get_server, register_server, remove_server, RpcCallback, RpcHandler, RpcServer, SERVERS,
};
pub use rpc_server_blockchain::RpcServerBlockchain;
pub use rpc_server_node::RpcServerNode;
pub use rpc_server_state::RpcServerState;
pub use rpc_server_utilities::RpcServerUtilities;
pub use rpc_server_wallet::RpcServerWallet;
pub use session::Session;

// Re-export smart contract handlers
pub use smart_contract::RpcServerSmartContract;

// Re-export WebSocket types
pub use ws::{SharedWsEventBridge, WsEvent, WsEventBridge, WsEventType, WsNotification};
