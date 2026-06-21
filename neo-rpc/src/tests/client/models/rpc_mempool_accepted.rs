use super::*;

#[test]
fn mempool_accepted_roundtrip() {
    let model = RpcMempoolAccepted {
        hashes: vec!["0x01".to_string(), "0x02".to_string()],
    };
    let json = serde_json::to_string(&model).unwrap();
    let parsed: RpcMempoolAccepted = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.hashes, model.hashes);
}
