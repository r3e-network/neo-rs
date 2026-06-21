use super::parsing::{
    insert_optional_string, object_array, optional_script_hash_or_address_lossy,
    parse_object_array_lossy, required_address_script_hash, required_bigint_string,
    required_script_hash_or_address, required_u16_number, required_u32_number, required_u64_number,
    required_uint256,
};
use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_primitives::{UInt160, UInt256};
use neo_serialization::json::{JObject, JToken};
use neo_wallets::wallet_helper::WalletAddress as WalletHelper;
use num_bigint::BigInt;

/// Builds the shared NEP balance container shape.
pub(crate) fn balance_list_to_json<T>(
    balances: &[T],
    user_script_hash: &UInt160,
    protocol_settings: &ProtocolSettings,
    mut to_json: impl FnMut(&T) -> JObject,
) -> JObject {
    let mut json = JObject::new();
    json.insert(
        "balance".to_string(),
        object_array(balances, |balance| to_json(balance)),
    );
    json.insert(
        "address".to_string(),
        JToken::String(WalletHelper::to_address(
            user_script_hash,
            protocol_settings.address_version,
        )),
    );
    json
}

/// Parses the shared NEP balance container shape.
pub(crate) fn parse_balance_list<T>(
    json: &JObject,
    protocol_settings: &ProtocolSettings,
    mut parse: impl FnMut(&JObject) -> CoreResult<T>,
) -> CoreResult<(Vec<T>, UInt160)> {
    let balances = parse_object_array_lossy(json, "balance", |obj| parse(obj));
    let user_script_hash = required_address_script_hash(json, "address", protocol_settings)?;
    Ok((balances, user_script_hash))
}

/// Shared NEP balance item fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NepBalanceFields {
    /// Balance amount.
    pub amount: BigInt,
    /// Last updated block height.
    pub last_updated_block: u32,
}

/// Borrowed NEP balance item fields used when building RPC JSON.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NepBalanceFieldRefs<'a> {
    /// Balance amount.
    pub amount: &'a BigInt,
    /// Last updated block height.
    pub last_updated_block: u32,
}

/// Appends shared NEP balance item fields in RPC wire order.
pub(crate) fn insert_nep_balance_fields(json: &mut JObject, fields: NepBalanceFieldRefs<'_>) {
    json.insert(
        "amount".to_string(),
        JToken::String(fields.amount.to_string()),
    );
    json.insert(
        "lastupdatedblock".to_string(),
        JToken::Number(f64::from(fields.last_updated_block)),
    );
}

/// Parses shared NEP balance item fields.
pub(crate) fn parse_nep_balance_fields(json: &JObject) -> CoreResult<NepBalanceFields> {
    Ok(NepBalanceFields {
        amount: required_bigint_string(json, "amount", "amount")
            .map_err(|e| CoreError::other(e.to_string()))?,
        last_updated_block: required_u32_number(json, "lastupdatedblock")
            .map_err(|e| CoreError::other(e.to_string()))?,
    })
}

/// Builds the shared NEP transfer container shape.
pub(crate) fn transfer_lists_to_json<T>(
    sent: &[T],
    received: &[T],
    user_script_hash: &UInt160,
    protocol_settings: &ProtocolSettings,
    mut to_json: impl FnMut(&T, &ProtocolSettings) -> JObject,
) -> JObject {
    let mut json = JObject::new();
    json.insert(
        "sent".to_string(),
        object_array(sent, |transfer| to_json(transfer, protocol_settings)),
    );
    json.insert(
        "received".to_string(),
        object_array(received, |transfer| to_json(transfer, protocol_settings)),
    );
    json.insert(
        "address".to_string(),
        JToken::String(WalletHelper::to_address(
            user_script_hash,
            protocol_settings.address_version,
        )),
    );
    json
}

/// Parses the shared NEP transfer container shape.
pub(crate) fn parse_transfer_lists<T>(
    json: &JObject,
    protocol_settings: &ProtocolSettings,
    mut parse: impl FnMut(&JObject, &ProtocolSettings) -> CoreResult<T>,
) -> CoreResult<(Vec<T>, Vec<T>, UInt160)> {
    let sent = parse_object_array_lossy(json, "sent", |obj| parse(obj, protocol_settings));
    let received = parse_object_array_lossy(json, "received", |obj| parse(obj, protocol_settings));
    let user_script_hash = required_address_script_hash(json, "address", protocol_settings)?;
    Ok((sent, received, user_script_hash))
}

/// Shared NEP transfer entry fields used by NEP-11 and NEP-17 RPC payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NepTransferFields {
    /// Timestamp in milliseconds.
    pub timestamp_ms: u64,
    /// Asset hash.
    pub asset_hash: UInt160,
    /// Optional transfer address script hash.
    pub user_script_hash: Option<UInt160>,
    /// Transfer amount.
    pub amount: BigInt,
    /// Block index.
    pub block_index: u32,
    /// Transfer notify index.
    pub transfer_notify_index: u16,
    /// Transaction hash.
    pub tx_hash: UInt256,
}

/// Borrowed NEP transfer entry fields used when building RPC JSON.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NepTransferFieldRefs<'a> {
    /// Timestamp in milliseconds.
    pub timestamp_ms: u64,
    /// Asset hash.
    pub asset_hash: UInt160,
    /// Optional transfer address script hash.
    pub user_script_hash: Option<UInt160>,
    /// Transfer amount.
    pub amount: &'a BigInt,
    /// Block index.
    pub block_index: u32,
    /// Transfer notify index.
    pub transfer_notify_index: u16,
    /// Transaction hash.
    pub tx_hash: UInt256,
}

/// Appends the shared NEP transfer entry fields in RPC wire order.
pub(crate) fn insert_nep_transfer_fields(
    json: &mut JObject,
    fields: NepTransferFieldRefs<'_>,
    protocol_settings: &ProtocolSettings,
) {
    json.insert(
        "timestamp".to_string(),
        JToken::Number(fields.timestamp_ms as f64),
    );
    json.insert(
        "assethash".to_string(),
        JToken::String(fields.asset_hash.to_string()),
    );

    insert_optional_string(
        json,
        "transferaddress",
        fields
            .user_script_hash
            .as_ref()
            .map(|hash| WalletHelper::to_address(hash, protocol_settings.address_version)),
    );

    json.insert(
        "amount".to_string(),
        JToken::String(fields.amount.to_string()),
    );
    json.insert(
        "blockindex".to_string(),
        JToken::Number(f64::from(fields.block_index)),
    );
    json.insert(
        "transfernotifyindex".to_string(),
        JToken::Number(f64::from(fields.transfer_notify_index)),
    );
    json.insert(
        "txhash".to_string(),
        JToken::String(fields.tx_hash.to_string()),
    );
}

/// Parses the shared NEP transfer entry fields while preserving legacy field semantics.
pub(crate) fn parse_nep_transfer_fields(
    json: &JObject,
    protocol_settings: &ProtocolSettings,
) -> CoreResult<NepTransferFields> {
    Ok(NepTransferFields {
        timestamp_ms: required_u64_number(json, "timestamp")
            .map_err(|e| CoreError::other(e.to_string()))?,
        asset_hash: required_script_hash_or_address(
            json,
            "assethash",
            protocol_settings,
            "asset hash",
        )
        .map_err(|e| CoreError::other(e.to_string()))?,
        user_script_hash: optional_script_hash_or_address_lossy(
            json,
            "transferaddress",
            protocol_settings,
        ),
        amount: required_bigint_string(json, "amount", "amount")
            .map_err(|e| CoreError::other(e.to_string()))?,
        block_index: required_u32_number(json, "blockindex")
            .map_err(|e| CoreError::other(e.to_string()))?,
        transfer_notify_index: required_u16_number(json, "transfernotifyindex")
            .map_err(|e| CoreError::other(e.to_string()))?,
        tx_hash: required_uint256(json, "txhash").map_err(|e| CoreError::other(e.to_string()))?,
    })
}

#[cfg(test)]
#[path = "../../tests/client/utility/nep.rs"]
mod tests;
