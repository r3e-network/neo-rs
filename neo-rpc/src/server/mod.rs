//! # neo-rpc::server
//!
//! JSON-RPC server composition, handlers, transports, and plugins.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `diagnostic`: RPC diagnostic endpoints and health reporting helpers.
//! - `dispatch`: RPC method dispatch, registration, and handler lookup helpers.
//! - `jsonrpsee_adapter`: jsonrpsee integration that exposes the internal RPC
//!   registry.
//! - `ledger_queries`: Shared ledger query helpers used by RPC handlers.
//! - `middleware`: RPC middleware for transport-level policy and observability.
//! - `model`: RPC request parameter models and conversion helpers.
//! - `native_queries`: Shared native-contract query helpers used by RPC
//!   handlers.
//! - `parameter_converter`: RPC parameter parsing and type conversion helpers.
//! - `rpc_error`: RPC error records exposed by the server boundary.
//! - `rpc_error_factory`: Helpers for constructing canonical RPC errors.
//! - `rpc_exception`: Exception-style RPC error wrappers used by handlers.
//! - `rpc_handler_macros`: Macros that bind typed RPC handlers into the
//!   registry.
//! - `rpc_helpers`: Shared helper functions for RPC handler implementations.
//! - `rpc_method_attribute`: RPC method descriptors and metadata.
//! - `rpc_registry`: RPC server registry and method table.
//! - `rpc_relay`: Relay helpers that submit transactions through the node
//!   boundary.
//! - `rpc_remote_ledger`: Remote-ledger RPC client used by RPC-only node mode.
//! - `rpc_server`: Core RPC server trait and callback registry.
//! - `rpc_server_application_logs`: Application-log RPC endpoint handlers.
//! - `rpc_server_blockchain`: Blockchain RPC endpoint handlers.
//! - `rpc_server_indexer`: Indexer-backed RPC endpoint handlers.
//! - `rpc_server_node`: Node and network RPC endpoint handlers.
//! - `rpc_server_oracle`: Oracle RPC endpoint handlers.
//! - `rpc_server_settings`: RPC server settings and configuration records.
//! - `rpc_server_state`: State-service RPC endpoint handlers.
//! - `rpc_server_tokens_tracker`: Token tracker RPC endpoint handlers.
//! - `rpc_server_utilities`: Utility RPC endpoint handlers.
//! - `rpc_server_wallet`: Wallet compatibility RPC endpoint handlers.
//! - `rpc_tls`: TLS configuration helpers for RPC transports.
//! - `rpc_transport`: RPC transport startup, binding, and shutdown helpers.
//! - `session`: RPC session records and connection-local state.
//! - `smart_contract`: Smart-contract RPC endpoint handlers.
//! - `test_support`: crate-local test support fixtures.
//! - `wallet_compat`: Wallet compatibility helpers for RPC responses.
//! - `ws`: WebSocket events, bridges, and notification models.

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
mod rpc_remote_ledger;
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
#[path = "../tests/server/support/test_support.rs"]
pub(crate) mod test_support;
mod wallet_compat;
pub mod ws;

// Public exports
pub use rpc_error::RpcError as ServerRpcError;
pub use rpc_exception::RpcException;
pub(crate) use rpc_handler_macros::rpc_handlers;
pub use rpc_method_attribute::RpcMethodDescriptor;
pub use rpc_registry::{SERVERS, ServerRegistry};
pub use rpc_remote_ledger::RemoteLedgerRpcClient;
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
