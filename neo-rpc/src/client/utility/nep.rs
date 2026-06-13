use super::parsing::{
    insert_optional_string, object_array, optional_script_hash_or_address_lossy,
    parse_object_array_lossy, required_address_script_hash, required_bigint_string,
    required_script_hash_or_address, required_u16_number, required_u32_number, required_u64_number,
    required_uint256,
};
use neo_config::ProtocolSettings;
use neo_primitives::{UInt160, UInt256};
use neo_serialization::json::{JObject, JToken};
use neo_wallets::wallet_helper as WalletHelper;
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
    mut parse: impl FnMut(&JObject) -> Result<T, String>,
) -> Result<(Vec<T>, UInt160), String> {
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
pub(crate) fn parse_nep_balance_fields(json: &JObject) -> Result<NepBalanceFields, String> {
    Ok(NepBalanceFields {
        amount: required_bigint_string(json, "amount", "amount")?,
        last_updated_block: required_u32_number(json, "lastupdatedblock")?,
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
    mut parse: impl FnMut(&JObject, &ProtocolSettings) -> Result<T, String>,
) -> Result<(Vec<T>, Vec<T>, UInt160), String> {
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
) -> Result<NepTransferFields, String> {
    Ok(NepTransferFields {
        timestamp_ms: required_u64_number(json, "timestamp")?,
        asset_hash: required_script_hash_or_address(
            json,
            "assethash",
            protocol_settings,
            "asset hash",
        )?,
        user_script_hash: optional_script_hash_or_address_lossy(
            json,
            "transferaddress",
            protocol_settings,
        ),
        amount: required_bigint_string(json, "amount", "amount")?,
        block_index: required_u32_number(json, "blockindex")?,
        transfer_notify_index: required_u16_number(json, "transfernotifyindex")?,
        tx_hash: required_uint256(json, "txhash")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balance_list_to_json_keeps_balance_before_address() {
        let settings = ProtocolSettings::default_settings();
        let token = balance_list_to_json(&[1_u8], &UInt160::zero(), &settings, |_| {
            let mut item = JObject::new();
            item.insert("value".to_string(), JToken::String("ok".to_string()));
            item
        });

        assert_eq!(
            token.to_string(),
            format!(
                r#"{{"balance":[{{"value":"ok"}}],"address":"{}"}}"#,
                WalletHelper::to_address(&UInt160::zero(), settings.address_version)
            )
        );
    }

    #[test]
    fn nep_balance_fields_preserve_string_amount_and_numeric_height() {
        let amount = BigInt::from(42);
        let mut json = JObject::new();
        insert_nep_balance_fields(
            &mut json,
            NepBalanceFieldRefs {
                amount: &amount,
                last_updated_block: 7,
            },
        );

        assert_eq!(json.to_string(), r#"{"amount":"42","lastupdatedblock":7}"#);

        let parsed = parse_nep_balance_fields(&json).expect("balance fields");
        assert_eq!(parsed.amount, amount);
        assert_eq!(parsed.last_updated_block, 7);
    }

    #[test]
    fn nep_balance_fields_preserve_legacy_type_errors() {
        let mut numeric_amount = JObject::new();
        numeric_amount.insert("amount".to_string(), JToken::Number(1.0));
        numeric_amount.insert("lastupdatedblock".to_string(), JToken::Number(7.0));
        assert_eq!(
            parse_nep_balance_fields(&numeric_amount).expect_err("numeric amount"),
            "Missing or invalid 'amount' field"
        );

        let mut string_height = JObject::new();
        string_height.insert("amount".to_string(), JToken::String("1".to_string()));
        string_height.insert(
            "lastupdatedblock".to_string(),
            JToken::String("7".to_string()),
        );
        assert_eq!(
            parse_nep_balance_fields(&string_height)
                .expect("string numeric height")
                .last_updated_block,
            7
        );

        let mut invalid_height = JObject::new();
        invalid_height.insert("amount".to_string(), JToken::String("1".to_string()));
        invalid_height.insert(
            "lastupdatedblock".to_string(),
            JToken::String("bad".to_string()),
        );
        assert_eq!(
            parse_nep_balance_fields(&invalid_height).expect_err("invalid height"),
            "Missing or invalid 'lastupdatedblock' field"
        );

        let mut invalid_amount = JObject::new();
        invalid_amount.insert("amount".to_string(), JToken::String("bad".to_string()));
        invalid_amount.insert("lastupdatedblock".to_string(), JToken::Number(7.0));
        assert_eq!(
            parse_nep_balance_fields(&invalid_amount).expect_err("invalid amount"),
            "Invalid amount: bad"
        );
    }
}
