//! Smart contract RPC endpoints (`RpcServer.SmartContract.cs` parity subset).

mod contract_verify;
mod helpers;
mod invocation;
mod iterators;
mod unclaimed_gas;

#[cfg(test)]
mod tests;

use crate::server::rpc_server::RpcHandler;

pub struct RpcServerSmartContract;

impl RpcServerSmartContract {
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
