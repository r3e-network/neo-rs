use neo_primitives::RpcException;

#[test]
fn rpc_exception_new_creates_without_data() {
    let exc = RpcException::new(-32600, "Invalid Request");
    assert_eq!(exc.code(), -32600);
    assert_eq!(exc.message(), "Invalid Request");
    assert_eq!(exc.data(), None);
}

#[test]
fn rpc_exception_with_data_creates_with_data() {
    let exc = RpcException::with_data(-32601, "Method not found", "method: test");
    assert_eq!(exc.code(), -32601);
    assert_eq!(exc.message(), "Method not found");
    assert_eq!(exc.data(), Some("method: test"));
}

#[test]
fn rpc_exception_from_parts_with_some_data() {
    let exc = RpcException::from_parts(-32602, "Invalid params", Some("param x is missing".to_string()));
    assert_eq!(exc.code(), -32602);
    assert_eq!(exc.message(), "Invalid params");
    assert_eq!(exc.data(), Some("param x is missing"));
}

#[test]
fn rpc_exception_from_parts_with_none_data() {
    let exc = RpcException::from_parts(-32603, "Internal error", None);
    assert_eq!(exc.code(), -32603);
    assert_eq!(exc.message(), "Internal error");
    assert_eq!(exc.data(), None);
}

#[test]
fn rpc_exception_from_parts_trims_whitespace() {
    let exc = RpcException::from_parts(-32700, "Parse error", Some("  \n  ".to_string()));
    assert_eq!(exc.data(), None); // Whitespace-only data becomes None
}

#[test]
fn rpc_exception_from_parts_trims_but_keeps_content() {
    let exc = RpcException::from_parts(-32700, "Parse error", Some("  data content  ".to_string()));
    assert_eq!(exc.data(), Some("data content"));
}

#[test]
fn rpc_exception_display_without_data() {
    let exc = RpcException::new(-32600, "Invalid Request");
    assert_eq!(exc.to_string(), "Invalid Request");
}

#[test]
fn rpc_exception_display_with_data() {
    let exc = RpcException::with_data(-32601, "Method not found", "extra info");
    assert_eq!(exc.to_string(), "Method not found - extra info");
}

#[test]
fn rpc_exception_clone() {
    let exc = RpcException::with_data(-32600, "Invalid", "data");
    let cloned = exc.clone();
    assert_eq!(exc.code(), cloned.code());
    assert_eq!(exc.message(), cloned.message());
    assert_eq!(exc.data(), cloned.data());
}

#[test]
fn rpc_exception_equality() {
    let exc1 = RpcException::new(-32600, "Invalid Request");
    let exc2 = RpcException::new(-32600, "Invalid Request");
    let exc3 = RpcException::new(-32601, "Invalid Request");

    assert_eq!(exc1, exc2);
    assert_ne!(exc1, exc3);
}

#[test]
fn rpc_exception_equality_with_data() {
    let exc1 = RpcException::with_data(-32600, "Invalid", "data");
    let exc2 = RpcException::with_data(-32600, "Invalid", "data");
    let exc3 = RpcException::with_data(-32600, "Invalid", "other");

    assert_eq!(exc1, exc2);
    assert_ne!(exc1, exc3);
}

#[test]
fn rpc_exception_debug_format() {
    let exc = RpcException::new(-32600, "Invalid Request");
    let debug = format!("{:?}", exc);
    assert!(debug.contains("RpcException"));
}

#[test]
fn rpc_exception_positive_error_codes() {
    let exc = RpcException::new(1, "Custom error");
    assert_eq!(exc.code(), 1);
}

#[test]
fn rpc_exception_zero_error_code() {
    let exc = RpcException::new(0, "Success?");
    assert_eq!(exc.code(), 0);
}

#[test]
fn rpc_exception_empty_message() {
    let exc = RpcException::new(-32600, "");
    assert_eq!(exc.message(), "");
}

#[test]
fn rpc_exception_empty_data() {
    let exc = RpcException::from_parts(-32600, "Error", Some("".to_string()));
    assert_eq!(exc.data(), None); // Empty string becomes None after trimming
}

#[test]
fn rpc_exception_large_error_code() {
    let exc = RpcException::new(i32::MAX, "Max code");
    assert_eq!(exc.code(), i32::MAX);
}

#[test]
fn rpc_exception_minimum_error_code() {
    let exc = RpcException::new(i32::MIN, "Min code");
    assert_eq!(exc.code(), i32::MIN);
}

#[test]
fn rpc_exception_multiline_data() {
    let exc = RpcException::with_data(-32600, "Error", "line1\nline2\nline3");
    assert_eq!(exc.data(), Some("line1\nline2\nline3"));
    assert!(exc.to_string().contains("line1\nline2\nline3"));
}

#[test]
fn rpc_exception_unicode_in_message() {
    let exc = RpcException::new(-32600, "错误消息 🚨");
    assert_eq!(exc.message(), "错误消息 🚨");
}

#[test]
fn rpc_exception_unicode_in_data() {
    let exc = RpcException::with_data(-32600, "Error", "数据 ✓");
    assert_eq!(exc.data(), Some("数据 ✓"));
}
