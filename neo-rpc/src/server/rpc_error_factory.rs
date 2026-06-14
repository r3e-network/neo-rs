// Rust port of Neo.Plugins.RpcServer.RpcErrorFactory providing helper
// constructors for specialised `RpcError` instances.

use super::rpc_error::RpcError;

pub fn invalid_contract_verification(data: impl Into<String>) -> RpcError {
    RpcError::invalid_contract_verification().with_data(data)
}
