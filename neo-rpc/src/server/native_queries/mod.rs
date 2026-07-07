//! # neo-rpc::server::native_queries
//!
//! Shared native-contract query helpers used by RPC handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `execution`: Read-only native-call engine execution.
//! - `neo`: NEO native-token read probes.
//! - `registry`: Standard native-contract registry construction.
//! - `result`: Native stack-result decoding.
//! - `script`: C#-compatible dynamic-call script construction.

mod execution;
mod neo;
mod registry;
mod result;
mod script;

/// Engine-script probes for native-contract reads.
pub(crate) struct NativeQueries;
