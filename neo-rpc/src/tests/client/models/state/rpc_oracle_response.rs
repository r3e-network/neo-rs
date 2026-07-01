use super::*;

#[test]
fn oracle_response_roundtrip() {
    let resp = RpcOracleResponse {
        id: 42,
        code: 0x16,
        result: "aGVsbG8=".to_string(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: RpcOracleResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, resp.id);
    assert_eq!(parsed.code, resp.code);
    assert_eq!(parsed.result, resp.result);
}
