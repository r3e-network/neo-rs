//! # neo-rpc::client::rpc_client
//!
//! HTTP JSON-RPC client implementation and request hooks for neo-rpc.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `blockchain`: Blockchain-domain primitive records used across crates.
//! - `builder`: RPC client builder.
//! - `client`: Client-side adapters for remote services and RPC access.
//! - `helpers`: Shared helper functions for the surrounding module.
//! - `hooks`: RPC client hook helpers.
//! - `tokens`: RPC token API helpers.
//! - `transactions`: RPC transaction submission and lookup methods.
//! - `wallet`: wallet RPC client methods.
//! - `tests`: Module-local tests and regression coverage.

mod blockchain;
mod builder;
mod client;
mod helpers;
mod hooks;
mod tokens;
mod transactions;
mod wallet;

#[cfg(test)]
#[path = "../../tests/client/rpc_client.rs"]
mod tests;

use reqwest::{Client, Url};
use std::sync::Arc;
use std::time::Duration;

use neo_config::ProtocolSettings;

pub use builder::RpcClientBuilder;
pub use hooks::{RpcClientHooks, RpcRequestOutcome};

const MAX_JSON_NESTING: usize = 128;
const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// The RPC client to call NEO RPC methods
/// Matches C# `RpcClient`
#[derive(Clone)]
pub struct RpcClient {
    base_address: Url,
    http_client: Client,
    pub(crate) protocol_settings: Arc<ProtocolSettings>,
    request_timeout: Duration,
    hooks: RpcClientHooks,
}
