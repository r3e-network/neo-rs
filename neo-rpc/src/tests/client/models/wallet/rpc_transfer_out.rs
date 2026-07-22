use super::*;
use crate::types::test_fixtures::rpc_case_params;
use neo_serialization::json::JArray;

#[test]
fn rpc_transfer_out_roundtrip() {
    let settings = ProtocolSettings::default_settings();
    let asset = UInt160::parse("0102030405060708090a0b0c0d0e0f1011121314").unwrap();
    let script_hash = UInt160::parse("1112131415161718191a1b1c1d1e1f2021222324").unwrap();

    let transfer = RpcTransferOut {
        asset,
        script_hash,
        value: "42".to_string(),
    };

    let mut json = transfer.to_json(&settings);
    json.insert(
        "asset".to_string(),
        JToken::String(format!("0X{}", strip_hex_prefix(&asset.to_string()))),
    );
    json.insert(
        "address".to_string(),
        JToken::String(format!("0X{}", strip_hex_prefix(&script_hash.to_string()))),
    );
    let parsed = RpcTransferOut::from_json(&json, &settings).expect("parse");

    assert_eq!(parsed.asset, transfer.asset);
    assert_eq!(parsed.script_hash, transfer.script_hash);
    assert_eq!(parsed.value, transfer.value);
}

#[test]
fn rpc_transfer_out_accepts_address_for_asset() {
    let settings = ProtocolSettings::default_settings();
    let script_hash = UInt160::parse("1112131415161718191a1b1c1d1e1f2021222324").unwrap();
    let mut json = JObject::new();
    let asset_address = WalletHelper::to_address(&UInt160::zero(), settings.address_version);
    json.insert("asset".to_string(), JToken::String(asset_address.clone()));
    json.insert("value".to_string(), JToken::String("1".to_string()));
    json.insert(
        "address".to_string(),
        JToken::String(WalletHelper::to_address(
            &script_hash,
            settings.address_version,
        )),
    );

    let parsed = RpcTransferOut::from_json(&json, &settings).expect("parse");
    assert_eq!(
        parsed.asset,
        WalletHelper::to_script_hash(&asset_address, settings.address_version).unwrap()
    );
    assert_eq!(parsed.script_hash, script_hash);
}

#[test]
fn rpc_transfer_out_accepts_scripthash_field() {
    let asset = UInt160::parse("0102030405060708090a0b0c0d0e0f1011121314").unwrap();
    let script_hash = UInt160::parse("1112131415161718191a1b1c1d1e1f2021222324").unwrap();

    let mut json = JObject::new();
    json.insert("asset".to_string(), JToken::String(asset.to_string()));
    json.insert("value".to_string(), JToken::String("5".to_string()));
    json.insert(
        "scripthash".to_string(),
        JToken::String(script_hash.to_string()),
    );

    let parsed =
        RpcTransferOut::from_json(&json, &ProtocolSettings::default_settings()).expect("parse");
    assert_eq!(parsed.script_hash, script_hash);
}

#[test]
fn transfer_out_to_json_matches_rpc_test_case() {
    let settings = ProtocolSettings::default_settings();
    let Some(params) = rpc_case_params("sendmanyasync") else {
        return;
    };
    let transfers = params
        .get(1)
        .and_then(|value| value.as_array())
        .expect("transfer outputs array");
    let parsed = transfers
        .children()
        .iter()
        .filter_map(|entry| entry.as_ref())
        .filter_map(|token| token.as_object())
        .filter_map(|obj| RpcTransferOut::from_json(obj, &settings).ok())
        .collect::<Vec<_>>();
    let actual = JArray::from(
        parsed
            .iter()
            .map(|transfer| JToken::Object(transfer.to_json(&settings)))
            .collect::<Vec<_>>(),
    );
    assert_eq!(transfers.to_string(), actual.to_string());
}
