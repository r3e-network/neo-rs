use neo_execution::helper::Helper as ContractHelper;
use neo_payloads::signer::Signer;
use neo_primitives::UInt160;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;

pub(super) struct TransferParamLayout {
    pub(super) method: &'static str,
    pub(super) from_index: Option<usize>,
    pub(super) to_index: usize,
    pub(super) amount_index: usize,
    pub(super) signers_index: usize,
}

pub(super) struct TransferRequest {
    pub(super) asset: UInt160,
    pub(super) from: Option<UInt160>,
    pub(super) to: UInt160,
    pub(super) amount: String,
    pub(super) signers: Option<Vec<Signer>>,
}

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
