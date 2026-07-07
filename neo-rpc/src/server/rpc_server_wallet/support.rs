use neo_execution::helper::Helper as ContractHelper;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;

pub(super) fn signature_contract_pubkey(script: &[u8]) -> Result<Vec<u8>, RpcException> {
    if !ContractHelper::is_signature_contract(script) {
        return Err(RpcException::from(
            RpcError::invalid_params().with_data("Unsupported contract script for signing"),
        ));
    }

    if script.len() < 35 {
        return Err(RpcException::from(
            RpcError::invalid_params().with_data("Invalid signature contract script"),
        ));
    }

    Ok(script[2..35].to_vec())
}
