//! # neo-rpc::server::rpc_server_tokens_tracker
//!
//! Token tracker RPC endpoint handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `balances`: NEP-11 and NEP-17 account-balance handlers.
//! - `helpers`: Shared helper functions for the surrounding module.
//! - `properties`: NEP-11 token property handler.
//! - `request`: Typed JSON-RPC request parsing helpers.
//! - `response`: Typed JSON-RPC response construction helpers.
//! - `transfers`: NEP-11 and NEP-17 transfer-history handlers.
//! - `tests`: Module-local tests and regression coverage.

use crate::server::rpc_server::RpcHandler;

mod balances;
mod helpers;
mod properties;
mod request;
mod response;
#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_tokens_tracker.rs"]
mod tests;
mod transfers;

/// RPC handler group for NEP-11 and NEP-17 token tracker methods.
pub struct RpcServerTokensTracker;

impl RpcServerTokensTracker {
    /// Register token tracker RPC handlers.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "getnep11balances" => Self::get_nep11_balances,
            "getnep11transfers" => Self::get_nep11_transfers,
            "getnep11properties" => Self::get_nep11_properties,
            "getnep17balances" => Self::get_nep17_balances,
            "getnep17transfers" => Self::get_nep17_transfers,
        ]
    }
}
