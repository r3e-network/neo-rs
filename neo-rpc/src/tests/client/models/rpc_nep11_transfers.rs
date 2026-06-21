use super::*;

#[test]
fn transfer_roundtrip() {
    let settings = ProtocolSettings::default_settings();
    let entry = RpcNep11Transfer {
        token_id: vec![1, 2, 3],
        timestamp_ms: 1234,
        asset_hash: UInt160::zero(),
        user_script_hash: Some(UInt160::zero()),
        amount: BigInt::from(7),
        block_index: 9,
        transfer_notify_index: 1,
        tx_hash: UInt256::zero(),
    };
    let mut json = entry.to_json(&settings);
    json.insert(
        "tokenid".to_string(),
        JToken::String("0X010203".to_string()),
    );
    let parsed = RpcNep11Transfer::from_json(&json, &settings).unwrap();
    assert_eq!(parsed.token_id, entry.token_id);
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
    let settings = ProtocolSettings::default_settings();
    let entry = RpcNep11Transfer {
        token_id: vec![0xaa],
        timestamp_ms: 1,
        asset_hash: UInt160::zero(),
        user_script_hash: None,
        amount: BigInt::from(11),
        block_index: 2,
        transfer_notify_index: 0,
        tx_hash: UInt256::zero(),
    };
    let transfers = RpcNep11Transfers {
        user_script_hash: UInt160::zero(),
        sent: vec![entry.clone()],
        received: vec![entry.clone()],
    };
    let json = transfers.to_json(&settings);
    let parsed = RpcNep11Transfers::from_json(&json, &settings).unwrap();
    assert_eq!(parsed.user_script_hash, transfers.user_script_hash);
    assert_eq!(parsed.sent.len(), 1);
    assert_eq!(parsed.received.len(), 1);
    assert_eq!(parsed.sent[0].token_id, entry.token_id);
    assert_eq!(parsed.received[0].user_script_hash, entry.user_script_hash);
}

#[test]
fn transfer_to_json_keeps_token_id_before_shared_fields() {
    let entry = RpcNep11Transfer {
        token_id: vec![1, 2, 3],
        timestamp_ms: 1234,
        asset_hash: UInt160::zero(),
        user_script_hash: None,
        amount: BigInt::from(7),
        block_index: 9,
        transfer_notify_index: 1,
        tx_hash: UInt256::zero(),
    };

    assert_eq!(
        entry
            .to_json(&ProtocolSettings::default_settings())
            .to_string(),
        format!(
            r#"{{"tokenid":"010203","timestamp":1234,"assethash":"{}","transferaddress":null,"amount":"7","blockindex":9,"transfernotifyindex":1,"txhash":"{}"}}"#,
            UInt160::zero(),
            UInt256::zero()
        )
    );
}
