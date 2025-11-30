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
//! use neo_rpc::client::RpcClient;
//!
//! let client = RpcClient::new("http://localhost:10332");
//! let block_count = client.get_block_count().await?;
//! ```

pub mod error;
pub mod error_code;

// Re-exports
pub use error::{RpcError, RpcResult};
pub use error_code::RpcErrorCode;

// Placeholder for future modules
// #[cfg(feature = "server")]
// pub mod server;
// #[cfg(feature = "client")]
// pub mod client;
// pub mod models;
