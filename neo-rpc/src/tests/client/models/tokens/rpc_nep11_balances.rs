use super::*;
use neo_serialization::json::JArray;
use neo_wallets::wallet_helper::WalletAddress as WalletHelper;

#[test]
fn token_balance_roundtrip() {
    let entry = RpcNep11TokenBalance {
        token_id: vec![0x01],
        amount: BigInt::from(42),
        last_updated_block: 7,
    };
    let mut json = entry.to_json();
    json.insert("tokenid".to_string(), JToken::String("0X01".to_string()));
    let parsed = RpcNep11TokenBalance::from_json(&json).unwrap();
    assert_eq!(parsed.token_id, entry.token_id);
    assert_eq!(parsed.amount, entry.amount);
    assert_eq!(parsed.last_updated_block, entry.last_updated_block);
}

#[test]
fn token_balance_to_json_keeps_tokenid_before_shared_fields() {
    let entry = RpcNep11TokenBalance {
        token_id: vec![0x01, 0x02],
        amount: BigInt::from(42),
        last_updated_block: 7,
    };

    assert_eq!(
        entry.to_json().to_string(),
        r#"{"tokenid":"0102","amount":"42","lastupdatedblock":7}"#
    );
}

#[test]
fn balances_roundtrip() {
    let settings = ProtocolSettings::default_settings();
    let entry = RpcNep11TokenBalance {
        token_id: vec![0x01],
        amount: BigInt::from(5),
        last_updated_block: 3,
    };
    let balance = RpcNep11Balance {
        asset_hash: UInt160::zero(),
        name: "Test".to_string(),
        symbol: "T".to_string(),
        decimals: 0,
        tokens: vec![entry.clone()],
    };
    let balances = RpcNep11Balances {
        user_script_hash: UInt160::zero(),
        balances: vec![balance.clone()],
    };
    let json = balances.to_json(&settings);
    let parsed = RpcNep11Balances::from_json(&json, &settings).unwrap();
    assert_eq!(parsed.user_script_hash, balances.user_script_hash);
    assert_eq!(parsed.balances.len(), 1);
    assert_eq!(parsed.balances[0].asset_hash, balance.asset_hash);
    assert_eq!(parsed.balances[0].tokens[0].amount, entry.amount);
}

#[test]
fn balances_and_tokens_arrays_keep_lossy_parse_behavior() {
    let settings = ProtocolSettings::default_settings();

    let valid_token = RpcNep11TokenBalance {
        token_id: vec![0x01],
        amount: BigInt::from(5),
        last_updated_block: 3,
    }
    .to_json();

    let mut malformed_token = JObject::new();
    malformed_token.insert("tokenid".to_string(), JToken::String("01".to_string()));

    let mut tokens = JArray::new();
    tokens.add(Some(JToken::Object(valid_token)));
    tokens.add(None);
    tokens.add(Some(JToken::String("not an object".to_string())));
    tokens.add(Some(JToken::Object(malformed_token)));

    let mut valid_balance = RpcNep11Balance {
        asset_hash: UInt160::zero(),
        name: "Test".to_string(),
        symbol: "T".to_string(),
        decimals: 0,
        tokens: Vec::new(),
    }
    .to_json();
    valid_balance.insert("tokens".to_string(), JToken::Array(tokens));

    let mut malformed_balance = JObject::new();
    malformed_balance.insert(
        "name".to_string(),
        JToken::String("missing hash".to_string()),
    );

    let mut balances = JArray::new();
    balances.add(Some(JToken::Object(valid_balance)));
    balances.add(None);
    balances.add(Some(JToken::String("not an object".to_string())));
    balances.add(Some(JToken::Object(malformed_balance)));

    let mut root = JObject::new();
    root.insert("balance".to_string(), JToken::Array(balances));
    root.insert(
        "address".to_string(),
        JToken::String(WalletHelper::to_address(
            &UInt160::zero(),
            settings.address_version,
        )),
    );

    let parsed = RpcNep11Balances::from_json(&root, &settings).unwrap();
    assert_eq!(parsed.balances.len(), 1);
    assert_eq!(parsed.balances[0].tokens.len(), 1);
    assert_eq!(parsed.balances[0].tokens[0].amount, BigInt::from(5));
}
