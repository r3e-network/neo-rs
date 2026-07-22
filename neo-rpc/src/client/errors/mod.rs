//! # neo-rpc::client::errors
//!
//! Errors produced by the Neo JSON-RPC client transport and protocol adapter.
//!
//! ## Boundary
//!
//! These errors describe outbound requests and remote JSON-RPC responses.
//! Server-side handler errors remain owned by `server::RpcException`.
//!
//! ## Contents
//!
//! - [`RpcClientError`]: client request, codec, and domain failures.
//! - [`ClientRpcError`]: an error returned by a remote JSON-RPC endpoint.

mod client;
mod protocol;

pub use client::{RpcClientError, RpcClientResult};
pub use protocol::ClientRpcError;
