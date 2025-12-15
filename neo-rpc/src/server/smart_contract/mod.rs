//! Smart contract RPC endpoints (`RpcServer.SmartContract.cs` parity subset).

mod contract_verify;
mod helpers;
mod invocation;
mod iterators;
mod unclaimed_gas;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use serde_json::Value;

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_method_attribute::RpcMethodDescriptor;
use crate::server::rpc_server::{RpcHandler, RpcServer};

pub struct RpcServerSmartContract;

impl RpcServerSmartContract {
    pub fn register_handlers() -> Vec<RpcHandler> {
        vec![
            Self::handler("invokefunction", invocation::invoke_function),
            Self::handler("invokescript", invocation::invoke_script),
            Self::handler(
                "invokecontractverify",
                contract_verify::invoke_contract_verify,
            ),
            Self::handler("traverseiterator", iterators::traverse_iterator),
            Self::handler("terminatesession", iterators::terminate_session),
            Self::handler("getunclaimedgas", unclaimed_gas::get_unclaimed_gas),
        ]
    }

    fn handler(
        name: &'static str,
        func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
    ) -> RpcHandler {
        RpcHandler::new(RpcMethodDescriptor::new(name), Arc::new(func))
    }
}
