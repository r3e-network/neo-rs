use super::*;
use crate::types::test_fixtures::rpc_case_result;
use neo_config::ProtocolSettings;

#[test]
fn transfer_roundtrip() {
    let entry = RpcNep17Transfer {
        timestamp_ms: 1234,
        asset_hash: UInt160::zero(),
        user_script_hash: Some(UInt160::zero()),
        amount: BigInt::from(7),
        block_index: 9,
        transfer_notify_index: 1,
        tx_hash: UInt256::zero(),
    };
    let json = entry.to_json(&ProtocolSettings::default_settings());
    let parsed = RpcNep17Transfer::from_json(&json, &ProtocolSettings::default_settings()).unwrap();
    assert_eq!(parsed.timestamp_ms, entry.timestamp_ms);
    assert_eq!(parsed.asset_hash, entry.asset_hash);
    assert_eq!(parsed.user_script_hash, entry.user_script_hash);
    assert_eq!(parsed.amount, entry.amount);
    assert_eq!(parsed.block_index, entry.block_index);
    assert_eq!(parsed.transfer_notify_index, entry.transfer_notify_index);
    assert_eq!(parsed.tx_hash, entry.tx_hash);
}

#[test]
fn transfers_roundtrip() {
    let entry = RpcNep17Transfer {
        timestamp_ms: 1,
        asset_hash: UInt160::zero(),
        user_script_hash: None,
        amount: BigInt::from(11),
        block_index: 2,
        transfer_notify_index: 0,
        tx_hash: UInt256::zero(),
    };
    let transfers = RpcNep17Transfers {
        user_script_hash: UInt160::zero(),
        sent: vec![entry.clone()],
        received: vec![entry.clone()],
    };
    let json = transfers.to_json(&ProtocolSettings::default_settings());
    let parsed =
        RpcNep17Transfers::from_json(&json, &ProtocolSettings::default_settings()).unwrap();

    assert_eq!(parsed.user_script_hash, transfers.user_script_hash);
    assert_eq!(parsed.sent.len(), 1);
    assert_eq!(parsed.received.len(), 1);
    assert_eq!(parsed.sent[0].amount, entry.amount);
    assert_eq!(parsed.received[0].user_script_hash, entry.user_script_hash);
}

#[test]
fn nep17_transfers_to_json_matches_rpc_test_case() {
    let Some(expected) = rpc_case_result("getnep17transfersasync") else {
        return;
    };
    let settings = ProtocolSettings::default_settings();
    let parsed = RpcNep17Transfers::from_json(&expected, &settings).expect("parse");
    let actual = parsed.to_json(&settings);
    assert_eq!(expected.to_string(), actual.to_string());
}

#[test]
fn nep17_transfers_null_address_matches_rpc_test_case() {
    let Some(expected) = rpc_case_result("getnep17transfersasync_with_null_transferaddress") else {
        return;
    };
    let settings = ProtocolSettings::default_settings();
    let parsed = RpcNep17Transfers::from_json(&expected, &settings).expect("parse");
    let actual = parsed.to_json(&settings);
    assert_eq!(expected.to_string(), actual.to_string());
}
