use std::error::Error;

use super::*;

#[test]
fn constructors_preserve_code_message_data_and_display() {
    let plain = RpcException::new(-32_600, "Invalid request");
    assert_eq!(plain.code(), -32_600);
    assert_eq!(plain.message(), "Invalid request");
    assert_eq!(plain.data(), None);
    assert_eq!(plain.to_string(), "Invalid request");

    let detailed = RpcException::with_data(-32_602, "Invalid params", "  bad height  ");
    assert_eq!(detailed.code(), -32_602);
    assert_eq!(detailed.message(), "Invalid params");
    assert_eq!(detailed.data(), Some("bad height"));
    assert_eq!(detailed.to_string(), "Invalid params - bad height");

    let error: &dyn Error = &detailed;
    assert_eq!(error.to_string(), "Invalid params - bad height");
}

#[test]
fn from_parts_discards_only_empty_trimmed_data() {
    assert_eq!(
        RpcException::from_parts(-1, "empty", Some(" \t\n ".to_string())).data(),
        None
    );
    assert_eq!(
        RpcException::from_parts(-1, "data", Some(" value ".to_string())).data(),
        Some("value")
    );
}

#[test]
fn rpc_error_conversion_round_trip_preserves_all_fields() {
    let original = RpcError::new(-10_001, "handler failed", Some(" detail ".to_string()));
    let exception = RpcException::from(original.clone());
    assert_eq!(exception.code(), original.code());
    assert_eq!(exception.message(), original.message());
    assert_eq!(exception.data(), original.data());

    let restored = RpcError::from(exception);
    assert_eq!(restored, original);
}
