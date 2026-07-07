//! # neo-rpc::server::rpc_server_settings
//!
//! RPC server settings and configuration records.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `config`: `RpcServerConfig` schema, defaults, and redacted debug formatting.
//! - `gas`: C#-compatible GAS setting deserializers.
//! - `registry`: Process-wide settings loading, validation, and lookup.
//! - `tests`: Module-local tests and regression coverage.

// Rust translation of Neo.Plugins.RpcServer.RpcServerSettings and
// RpcServersSettings. Provides JSON configuration deserialisation for the RPC
// server plugin.

mod config;
mod gas;
mod registry;

pub use config::RpcServerConfig;
pub use registry::RpcServerSettings;
use serde::Deserialize;

/// Policy for handling unhandled exceptions in the RPC server
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum UnhandledExceptionPolicy {
    /// Ignore exceptions and continue processing
    #[default]
    Ignore,
    /// Log exceptions
    Log,
    /// Stop the plugin/service
    StopPlugin,
    /// Stop the node
    StopNode,
    /// Continue after logging
    Continue,
    /// Terminate the process
    Terminate,
}

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_settings.rs"]
mod tests;
