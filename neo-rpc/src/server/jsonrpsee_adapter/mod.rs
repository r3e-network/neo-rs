//! # neo-rpc::server::jsonrpsee_adapter
//!
//! jsonrpsee integration that exposes the internal RPC registry.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `auth`: transport authentication marker and Basic header verification.
//! - `codec`: transport parameter decoding and JSON-RPC error projection.
//! - `context`: shared jsonrpsee callback context.
//! - `dispatch`: per-request dispatch into the internal RPC registry.
//! - `module`: jsonrpsee module registration.

mod auth;
mod codec;
mod context;
mod dispatch;
mod module;

pub(crate) use auth::{RpcAuthState, verify_basic_auth_header};
pub use context::JsonRpseeContext;
pub use module::{
    build_jsonrpsee_module, build_jsonrpsee_module_with_disabled,
    build_jsonrpsee_module_with_methods,
};
