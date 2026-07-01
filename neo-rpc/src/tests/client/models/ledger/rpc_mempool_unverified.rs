use super::*;

#[test]
fn mempool_unverified_roundtrip() {
    let model = RpcMempoolUnverified {
        hashes: vec!["0x0a".to_string()],
    };
    let json = serde_json::to_string(&model).unwrap();
    let parsed: RpcMempoolUnverified = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.hashes, model.hashes);
}
