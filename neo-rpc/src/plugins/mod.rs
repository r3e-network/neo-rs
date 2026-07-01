//! # neo-rpc::plugins
//!
//! RPC plugin adapters and optional extension surfaces.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `tokens_tracker`: Token tracker plugin wiring and index-derived token
//!   views.

#[cfg(feature = "server")]
pub mod tokens_tracker;
