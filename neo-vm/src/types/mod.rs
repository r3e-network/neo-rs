//! # neo-vm::types
//!
//! Storage-domain types shared by store implementations.
//!
//! ## Boundary
//!
//! This module belongs to `neo-vm`. This VM crate owns deterministic script
//! execution and must not own ledger persistence, network transport, or node
//! composition.
//!
//! ## Contents
//!
//! - `contract`: verification contract script, parameter list, and cached hash.
//! - `error`: Typed error definitions and conversions.
//! - `rpc_json`: VM RPC JSON conversion helpers.
//! - `script`: NeoVM script record and byte helpers.

pub mod contract;
pub mod error;
pub mod rpc_json;
pub mod script;
