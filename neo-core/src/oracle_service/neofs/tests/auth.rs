use super::super::auth::{salt_message_wallet_connect, strip_bearer_prefix};
use super::super::json::normalize_neofs_hex_header;

#[test]
fn normalize_neofs_hex_header_strips_prefix() {
    assert_eq!(normalize_neofs_hex_header("0xdeadbeef"), "deadbeef");
}

#[test]
fn normalize_neofs_hex_header_converts_base64() {
    let input = "aGVsbG8=";
    assert_eq!(normalize_neofs_hex_header(input), "68656c6c6f");
}

#[test]
fn normalize_neofs_hex_header_preserves_hex() {
    let input = "cafebabe";
    assert_eq!(normalize_neofs_hex_header(input), "cafebabe");
}

#[test]
fn strip_bearer_prefix_handles_case() {
    assert_eq!(strip_bearer_prefix("Bearer token"), "token");
    assert_eq!(strip_bearer_prefix("bearer token"), "token");
    assert_eq!(strip_bearer_prefix("token"), "token");
}

#[test]
fn salt_message_wallet_connect_includes_salt_and_suffix() {
    let data = b"payload";
    let salt = [0x11u8; 16];
    let message = salt_message_wallet_connect(data, &salt);
    assert!(message.starts_with(&[0x01, 0x00, 0x01, 0xf0]));
    assert_eq!(&message[message.len() - 2..], [0x00, 0x00]);

    let salt_hex = hex::encode(salt);
    let salt_bytes = salt_hex.as_bytes();
    let contains_salt = message
        .windows(salt_bytes.len())
        .any(|window| window == salt_bytes);
    assert!(contains_salt, "salt hex not found in message");

    let contains_data = message.windows(data.len()).any(|window| window == data);
    assert!(contains_data, "data not found in message");
}
