//! # neo-rpc::server::rpc_error
//!
//! RPC error records exposed by the server boundary.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `catalog`: Named C#-compatible RPC error constructors.
//! - `record`: JSON-RPC error record, formatting, and JSON projection.
//! - `tests`: Module-local tests and regression coverage.

// This module mirrors Neo.Plugins.RpcServer.RpcError from the C# codebase while
// following idiomatic Rust patterns. It provides strongly-typed error instances
// that can be serialised to JSON responses for the RPC subsystem.

mod catalog;
mod record;

pub use record::RpcError;

#[cfg(test)]
#[path = "../../tests/server/core/rpc_error.rs"]
mod tests;
