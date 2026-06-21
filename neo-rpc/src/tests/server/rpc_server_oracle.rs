use super::*;

#[test]
fn map_oracle_error_includes_signature_message() {
    let err = OracleServiceError::InvalidSignature("bad signature".to_string());
    let rpc = map_oracle_error(err);
    assert_eq!(rpc.code(), RpcError::invalid_signature().code());
    assert_eq!(rpc.data(), Some("bad signature"));
}

#[test]
fn map_oracle_error_includes_not_designated_message() {
    let err = OracleServiceError::NotDesignated("not oracle".to_string());
    let rpc = map_oracle_error(err);
    assert_eq!(rpc.code(), RpcError::oracle_not_designated_node().code());
    assert_eq!(rpc.data(), Some("not oracle"));
}
