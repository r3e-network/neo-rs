//! # Neo RPC
//!
//! RPC server and client for the Neo blockchain.
//!
//! This crate provides a unified RPC implementation including:
//! - JSON-RPC server for node operations
//! - RPC client for interacting with Neo nodes
//! - Typed request/response models
//!
//! ## Features
//!
//! - `server`: Enable RPC server functionality
//! - `client`: Enable RPC client functionality
//!
//! ## Example
//!
//! ```rust,ignore
//! use neo_rpc::server::{RpcServer, RpcServerConfig};
//!
//! let config = RpcServerConfig::default();
//! let server = RpcServer::new(system, config);
//! server.start_rpc_server();
//! ```

pub mod error;
pub mod error_code;

#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "client")]
pub mod client;

// Re-exports
pub use error::{RpcError, RpcResult};
pub use error_code::RpcErrorCode;

#[cfg(feature = "server")]
pub use server::{RpcServer, RpcServerConfig, RpcServerSettings};

#[cfg(feature = "client")]
pub use client::{
    ClientRpcError, ContractClient, Nep17Api, PolicyApi, RpcClient, RpcClientBuilder,
    RpcClientHooks, RpcRequestOutcome, RpcUtility, StateApi, TransactionManager,
    TransactionManagerFactory, WalletApi,
};
