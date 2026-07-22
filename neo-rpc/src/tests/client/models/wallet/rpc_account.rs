use super::*;
use crate::types::test_fixtures::rpc_case_result;

#[test]
fn rpc_account_roundtrip_with_label() {
    let account = RpcAccount {
        address: "NQ7cbaBqX1p5quJDQr6b1qnBZBHae3mJzA".to_string(),
        has_key: true,
        label: Some("main".to_string()),
        watch_only: false,
    };

    let json = account.to_json();
    let parsed = RpcAccount::from_json(&json).expect("account");
    assert_eq!(parsed.address, account.address);
    assert_eq!(parsed.has_key, account.has_key);
    assert_eq!(parsed.label, account.label);
    assert_eq!(parsed.watch_only, account.watch_only);
}

#[test]
fn rpc_account_roundtrip_without_label() {
    let account = RpcAccount {
        address: "NQ7cbaBqX1p5quJDQr6b1qnBZBHae3mJzA".to_string(),
        has_key: false,
        label: None,
        watch_only: true,
    };

    let json = account.to_json();
    assert!(matches!(json.get("label"), Some(JToken::Null)));
    let parsed = RpcAccount::from_json(&json).expect("account");
    assert!(parsed.label.is_none());
    assert!(parsed.watch_only);
}

#[test]
fn account_to_json_matches_rpc_test_case() {
    let Some(expected) = rpc_case_result("importprivkeyasync") else {
        return;
    };
    let parsed = RpcAccount::from_json(&expected).expect("parse");
    let actual = parsed.to_json();
    assert_eq!(expected.to_string(), actual.to_string());
}
