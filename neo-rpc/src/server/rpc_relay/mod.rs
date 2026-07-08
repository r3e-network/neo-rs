//! # neo-rpc::server::rpc_relay
//!
//! Relay helpers that submit transactions through the node boundary.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `block`: block relay preflight and import orchestration.
//! - `ledger_provider`: Ledger read seam for relay preflight.
//! - `result`: C#-compatible relay-result projection into JSON-RPC responses.
//! - `runtime`: synchronous bridge for async service calls.
//! - `transaction`: transaction relay through mempool admission.

mod block;
mod ledger_provider;
mod result;
mod runtime;
mod transaction;

pub(super) use block::relay_block;
pub(super) use result::map_relay_result;
pub(super) use transaction::relay_transaction;
