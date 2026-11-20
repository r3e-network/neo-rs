//! Smart contract RPC endpoints (`RpcServer.SmartContract.cs` parity subset).

mod contract_verify;
mod helpers;
mod invocation;
mod iterators;
mod unclaimed_gas;

#[cfg(test)]
mod tests;

use crate::rpc_server::RpcHandler;

pub struct RpcServerSmartContract;

impl RpcServerSmartContract {
    /// Registers the smart-contract RPC handlers.
    ///
    /// # Security
    /// No handler in this module signs, so none is registered as protected.
    /// `invokefunction`/`invokescript` are read-only simulation and are
    /// wallet-free; `invokecontractverify` only runs a `verify` script against
    /// supplied witnesses. Any future method that builds a transaction or
    /// produces a witness must not live here: it belongs in the wallet module,
    /// behind `rpc_handlers![protected; ...]` and the unified signer service.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "invokefunction" => invocation::invoke_function,
            "invokescript" => invocation::invoke_script,
            "invokecontractverify" => contract_verify::invoke_contract_verify,
            "traverseiterator" => iterators::traverse_iterator,
            "terminatesession" => iterators::terminate_session,
            "getunclaimedgas" => unclaimed_gas::get_unclaimed_gas,
        ]
    }
}
