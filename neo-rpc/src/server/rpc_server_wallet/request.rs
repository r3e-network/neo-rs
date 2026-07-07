//! Typed request parsing for wallet RPC handlers.
//!
//! These structs keep JSON-RPC parameter decoding and C# compatibility error
//! text out of the handler bodies. Handlers should consume typed requests and
//! keep wallet, ledger, and relay work in the orchestration layer.

use neo_payloads::transaction::Transaction;
use neo_primitives::UInt160;
use serde_json::Value;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{
    expect_base64_param_with_decode_message, expect_string_param, invalid_params,
    parse_script_hash_or_address_with_error, parse_uint160,
};

pub(super) struct DumpPrivKeyRequest {
    pub(super) script_hash: UInt160,
}

impl DumpPrivKeyRequest {
    pub(super) fn parse(params: &[Value], address_version: u8) -> Result<Self, RpcException> {
        let address = expect_string_param(params, 0, "dumpprivkey")?;
        Ok(Self {
            script_hash: parse_wallet_script_hash(&address, address_version)?,
        })
    }
}

pub(super) struct WalletBalanceRequest {
    pub(super) asset: UInt160,
}

impl WalletBalanceRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        Ok(Self {
            asset: parse_uint160(params, 0, "getwalletbalance")?,
        })
    }
}

pub(super) struct ImportPrivKeyRequest {
    pub(super) wif: String,
}

impl ImportPrivKeyRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        Ok(Self {
            wif: expect_string_param(params, 0, "importprivkey")?,
        })
    }
}

pub(super) struct OpenWalletRequest {
    pub(super) path: String,
    pub(super) password: String,
}

impl OpenWalletRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        Ok(Self {
            path: expect_string_param(params, 0, "openwallet")?,
            password: expect_string_param(params, 1, "openwallet")?,
        })
    }
}

pub(super) struct NetworkFeeRequest {
    pub(super) transaction: Transaction,
}

impl NetworkFeeRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        let raw = expect_base64_param_with_decode_message(
            params,
            0,
            "calculatenetworkfee",
            "Invalid transaction payload",
        )?;
        let transaction = Transaction::from_bytes(&raw).map_err(|err| {
            RpcException::from(
                RpcError::invalid_params().with_data(format!("Invalid transaction: {err}")),
            )
        })?;
        Ok(Self { transaction })
    }
}

pub(super) fn parse_wallet_script_hash(
    value: &str,
    address_version: u8,
) -> Result<UInt160, RpcException> {
    parse_script_hash_or_address_with_error(value, address_version, |err| {
        invalid_params(err.to_string())
    })
}
