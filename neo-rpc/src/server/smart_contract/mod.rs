//! # neo-rpc::server::smart_contract
//!
//! Smart-contract RPC endpoint handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `contract_verify`: smart-contract verification handlers.
//! - `helpers`: Shared helper functions for the surrounding module.
//! - `invoke`: script construction, execution, diagnostics, and wallet follow-up.
//! - `iterators`: Iterator adapters exposed to contract execution and storage
//!   search.
//! - `native_provider`: Native-contract read seam used by invocation helpers.
//! - `request`: Typed request parsing for smart-contract handlers.
//! - `response`: VM-state, stack-item, and notification JSON projection.
//! - `unclaimed_gas`: unclaimed GAS query handlers.
//! - `tests`: Module-local tests and regression coverage.

mod contract_verify;
mod helpers;
mod invoke;
mod iterators;
mod native_provider;
mod request;
mod response;
mod unclaimed_gas;

#[cfg(test)]
#[path = "../../tests/server/services/smart_contract.rs"]
mod tests;

use crate::server::rpc_server::RpcHandler;

/// RPC handler group for smart-contract invocation methods.
pub struct RpcServerSmartContract;

impl RpcServerSmartContract {
    /// Register smart-contract RPC handlers.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "invokefunction" => invoke::invoke_function,
            "invokescript" => invoke::invoke_script,
            "invokecontractverify" => contract_verify::invoke_contract_verify,
            "traverseiterator" => iterators::traverse_iterator,
            "terminatesession" => iterators::terminate_session,
            "getunclaimedgas" => unclaimed_gas::get_unclaimed_gas,
        ]
    }
}
