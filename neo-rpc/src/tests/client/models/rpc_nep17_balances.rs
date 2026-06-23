use super::super::test_fixtures::rpc_case_result;
use super::*;
use neo_config::ProtocolSettings;
use neo_serialization::json::{JArray, JToken};
use neo_wallets::wallet_helper::WalletAddress as WalletHelper;

#[test]
fn balance_roundtrip() {
    let entry = RpcNep17Balance {
        asset_hash: UInt160::zero(),
        amount: BigInt::from(42),
        last_updated_block: 10,
    };
    let json = entry.to_json();
    let parsed = RpcNep17Balance::from_json(&json, &ProtocolSettings::default_settings()).unwrap();
    assert_eq!(parsed.asset_hash, entry.asset_hash);
    assert_eq!(parsed.amount, entry.amount);
    assert_eq!(parsed.last_updated_block, entry.last_updated_block);
}

#[test]
fn balances_roundtrip() {
    let entry = RpcNep17Balance {
        asset_hash: UInt160::zero(),
        amount: BigInt::from(5),
        last_updated_block: 3,
    };
    let balances = RpcNep17Balances {
        user_script_hash: UInt160::zero(),
        balances: vec![entry.clone()],
    };
    let json = balances.to_json(&ProtocolSettings::default_settings());
    let parsed = RpcNep17Balances::from_json(&json, &ProtocolSettings::default_settings()).unwrap();

    assert_eq!(parsed.user_script_hash, balances.user_script_hash);
    assert_eq!(parsed.balances.len(), 1);
    assert_eq!(parsed.balances[0].amount, entry.amount);
}

#[test]
fn balances_array_keeps_lossy_parse_behavior() {
    let settings = ProtocolSettings::default_settings();
    let valid = RpcNep17Balance {
        asset_hash: UInt160::zero(),
        amount: BigInt::from(5),
        last_updated_block: 3,
    }
    .to_json();

    let mut malformed = JObject::new();
    malformed.insert(
        "assethash".to_string(),
        JToken::String(UInt160::zero().to_string()),
    );

    let mut balances = JArray::new();
    balances.add(Some(JToken::Object(valid)));
    balances.add(None);
    balances.add(Some(JToken::String("not an object".to_string())));
    balances.add(Some(JToken::Object(malformed)));

    let mut root = JObject::new();
    root.insert("balance".to_string(), JToken::Array(balances));
    root.insert(
        "address".to_string(),
        JToken::String(WalletHelper::to_address(
            &UInt160::zero(),
            settings.address_version,
        )),
    );

    let parsed = RpcNep17Balances::from_json(&root, &settings).unwrap();
    assert_eq!(parsed.balances.len(), 1);
    assert_eq!(parsed.balances[0].amount, BigInt::from(5));
}

#[test]
fn nep17_balances_to_json_matches_rpc_test_case() {
    let Some(expected) = rpc_case_result("getnep17balancesasync") else {
        return;
    };
    let settings = ProtocolSettings::default_settings();
    let parsed = RpcNep17Balances::from_json(&expected, &settings).expect("parse");
    let actual = parsed.to_json(&settings);
    assert_eq!(expected.to_string(), actual.to_string());
}
