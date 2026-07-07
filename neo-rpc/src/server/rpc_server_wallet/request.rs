//! Typed request parsing for wallet RPC handlers.
//!
//! These structs keep JSON-RPC parameter decoding and C# compatibility error
//! text out of the handler bodies. Handlers should consume typed requests and
//! keep wallet, ledger, and relay work in the orchestration layer.

use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_primitives::{UInt160, UInt256, WitnessScope};
use serde_json::Value;
use std::str::FromStr;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{
    expect_base64_param_with_decode_message, expect_string_param, invalid_params,
    parse_script_hash_or_address_with_error, parse_uint160, parse_uint256,
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

pub(super) struct TransferRequest {
    pub(super) asset: UInt160,
    pub(super) from: Option<UInt160>,
    pub(super) to: UInt160,
    pub(super) amount: String,
    pub(super) signers: Option<Vec<Signer>>,
}

impl TransferRequest {
    pub(super) fn parse_send_from(
        params: &[Value],
        address_version: u8,
    ) -> Result<Self, RpcException> {
        Self::parse(
            params,
            address_version,
            TransferParamLayout {
                method: "sendfrom",
                from_index: Some(1),
                to_index: 2,
                amount_index: 3,
                signers_index: 4,
            },
        )
    }

    pub(super) fn parse_send_to_address(
        params: &[Value],
        address_version: u8,
    ) -> Result<Self, RpcException> {
        Self::parse(
            params,
            address_version,
            TransferParamLayout {
                method: "sendtoaddress",
                from_index: None,
                to_index: 1,
                amount_index: 2,
                signers_index: 3,
            },
        )
    }

    fn parse(
        params: &[Value],
        address_version: u8,
        layout: TransferParamLayout,
    ) -> Result<Self, RpcException> {
        let asset = parse_uint160(params, 0, layout.method)?;
        let from = layout
            .from_index
            .map(|index| {
                expect_string_param(params, index, layout.method)
                    .and_then(|text| parse_wallet_script_hash(&text, address_version))
            })
            .transpose()?;
        let to = parse_wallet_script_hash(
            &expect_string_param(params, layout.to_index, layout.method)?,
            address_version,
        )?;
        let amount = expect_string_param(params, layout.amount_index, layout.method)?;
        let signers = parse_optional_signers(params, layout.signers_index, address_version)?;

        Ok(Self {
            asset,
            from,
            to,
            amount,
            signers,
        })
    }
}

struct TransferParamLayout {
    method: &'static str,
    from_index: Option<usize>,
    to_index: usize,
    amount_index: usize,
    signers_index: usize,
}

pub(super) struct SendManyRequest {
    pub(super) from: Option<UInt160>,
    pub(super) outputs: Vec<SendManyOutputRequest>,
    pub(super) signers: Option<Vec<Signer>>,
}

impl SendManyRequest {
    pub(super) fn parse(params: &[Value], address_version: u8) -> Result<Self, RpcException> {
        if params.is_empty() {
            return Err(invalid_params("sendmany requires at least one argument"));
        }
        let mut from = None;
        let mut index = 0;
        if params[0].is_string() {
            from = Some(parse_wallet_script_hash(
                &expect_string_param(params, 0, "sendmany")?,
                address_version,
            )?);
            index = 1;
        }

        let outputs_value = params.get(index).cloned().unwrap_or(Value::Null);
        let outputs_array = outputs_value
            .as_array()
            .ok_or_else(|| invalid_params(format!("Invalid 'to' parameter: {outputs_value}")))?;
        if outputs_array.is_empty() {
            return Err(invalid_params("Argument 'to' can't be empty."));
        }

        let signers = parse_optional_signers(params, index + 1, address_version)?;
        let outputs = outputs_array
            .iter()
            .enumerate()
            .map(|(i, entry)| SendManyOutputRequest::parse(i, entry, address_version))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            from,
            outputs,
            signers,
        })
    }
}

pub(super) struct SendManyOutputRequest {
    pub(super) asset: UInt160,
    pub(super) value: Option<String>,
    pub(super) address: Option<String>,
}

impl SendManyOutputRequest {
    fn parse(index: usize, entry: &Value, _address_version: u8) -> Result<Self, RpcException> {
        let obj = entry
            .as_object()
            .ok_or_else(|| invalid_params(format!("Invalid 'to' parameter at {index}.")))?;
        let asset_str = obj
            .get("asset")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_params(format!("no 'asset' parameter at 'to[{index}]'.")))?;
        let asset = UInt160::from_str(asset_str)
            .map_err(|err| invalid_params(format!("invalid asset {asset_str}: {err}")))?;
        let value = obj.get("value").and_then(Value::as_str).map(str::to_string);
        let address = obj
            .get("address")
            .and_then(Value::as_str)
            .map(str::to_string);
        Ok(Self {
            asset,
            value,
            address,
        })
    }
}

pub(super) struct CancelTransactionRequest {
    pub(super) txid: UInt256,
    pub(super) signers: Vec<Signer>,
    pub(super) extra_fee: Option<String>,
}

impl CancelTransactionRequest {
    pub(super) fn parse(params: &[Value], address_version: u8) -> Result<Self, RpcException> {
        let txid = parse_uint256(params, 0, "canceltransaction")?;
        let signers_value = params
            .get(1)
            .ok_or_else(|| invalid_params("canceltransaction requires signers"))?;
        let signers_array = signers_value
            .as_array()
            .ok_or_else(|| invalid_params("canceltransaction signers must be an array"))?;
        if signers_array.is_empty() {
            return Err(RpcException::from(
                RpcError::bad_request().with_data("No signer."),
            ));
        }

        let signers = parse_signer_array(
            signers_array,
            address_version,
            "canceltransaction signers must be strings",
            WitnessScope::NONE,
        )?;
        let extra_fee = params.get(2).and_then(Value::as_str).map(str::to_string);
        Ok(Self {
            txid,
            signers,
            extra_fee,
        })
    }
}

pub(super) fn parse_signers(
    value: &Value,
    address_version: u8,
) -> Result<Vec<Signer>, RpcException> {
    let array = value
        .as_array()
        .ok_or_else(|| invalid_params("signers must be an array"))?;
    parse_signer_array(
        array,
        address_version,
        "signer entries must be strings",
        WitnessScope::CALLED_BY_ENTRY,
    )
}

pub(super) fn parse_signer_array(
    array: &[Value],
    address_version: u8,
    entry_error: &'static str,
    scope: WitnessScope,
) -> Result<Vec<Signer>, RpcException> {
    let mut signers = Vec::with_capacity(array.len());
    for entry in array {
        let addr = entry.as_str().ok_or_else(|| invalid_params(entry_error))?;
        let hash = parse_wallet_script_hash(addr, address_version)?;
        signers.push(Signer::new(hash, scope));
    }
    Ok(signers)
}

fn parse_optional_signers(
    params: &[Value],
    index: usize,
    address_version: u8,
) -> Result<Option<Vec<Signer>>, RpcException> {
    params
        .get(index)
        .map(|value| parse_signers(value, address_version))
        .transpose()
}

pub(super) fn parse_wallet_script_hash(
    value: &str,
    address_version: u8,
) -> Result<UInt160, RpcException> {
    parse_script_hash_or_address_with_error(value, address_version, |err| {
        invalid_params(err.to_string())
    })
}
