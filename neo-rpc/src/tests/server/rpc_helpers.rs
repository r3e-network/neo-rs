use super::*;

#[test]
fn expect_uint256_param_with_message_parses_valid_hash() {
    let hash = UInt256::zero();
    let params = [Value::String(hash.to_string())];

    let parsed = expect_uint256_param_with_message(&params, 0, "method expects hash", "hash")
        .expect("valid hash");

    assert_eq!(parsed, hash);
}

#[test]
fn expect_uint256_param_with_message_uses_custom_missing_message() {
    let err = expect_uint256_param_with_message(&[], 0, "method expects hash", "hash")
        .expect_err("missing hash");

    assert!(err.to_string().contains("method expects hash"), "{err}");
}

#[test]
fn expect_uint256_param_with_message_uses_custom_invalid_label() {
    let params = [Value::String("not-a-hash".to_string())];

    let err =
        expect_uint256_param_with_message(&params, 0, "method expects hash", "block hash")
            .expect_err("invalid hash");

    assert!(err.to_string().contains("invalid block hash"), "{err}");
}

#[test]
fn expect_uint160_param_with_message_parses_valid_hash() {
    let hash = UInt160::zero();
    let params = [Value::String(hash.to_string())];

    let parsed = expect_uint160_param_with_message(
        &params,
        0,
        "method expects script hash",
        "script hash",
    )
    .expect("valid script hash");

    assert_eq!(parsed, hash);
}

#[test]
fn expect_uint160_param_with_message_uses_custom_missing_message() {
    let err =
        expect_uint160_param_with_message(&[], 0, "method expects script hash", "script hash")
            .expect_err("missing script hash");

    assert!(
        err.to_string().contains("method expects script hash"),
        "{err}"
    );
}

#[test]
fn expect_uint160_param_with_message_uses_custom_invalid_label() {
    let params = [Value::String("not-a-script-hash".to_string())];

    let err = expect_uint160_param_with_message(
        &params,
        0,
        "method expects script hash",
        "script hash",
    )
    .expect_err("invalid script hash");

    assert!(err.to_string().contains("invalid script hash"), "{err}");
}

#[test]
fn parse_script_hash_or_address_accepts_wallet_address() {
    let hash = UInt160::zero();
    let address_version = 0x35;
    let address = neo_wallets::wallet_helper::WalletAddress::to_address(&hash, address_version);

    let parsed =
        parse_script_hash_or_address(&address, address_version).expect("valid address");

    assert_eq!(parsed, hash);
}

#[test]
fn parse_script_hash_or_address_with_error_uses_custom_address_error() {
    let err = parse_script_hash_or_address_with_error("not-an-address", 0x35, |_| {
        invalid_params("wallet address error")
    })
    .expect_err("invalid address");

    assert!(err.to_string().contains("wallet address error"), "{err}");
}

#[test]
fn expect_base64_param_with_messages_uses_custom_missing_message() {
    let err =
        expect_base64_param_with_messages(&[], 0, "method requires Base64 payload", |text| {
            format!("invalid Base64 payload: {text}")
        })
        .expect_err("missing base64");

    assert!(
        err.to_string().contains("method requires Base64 payload"),
        "{err}"
    );
}

#[test]
fn expect_base64_param_with_messages_uses_input_aware_decode_message() {
    let params = [Value::String("not-base64".to_string())];

    let err = expect_base64_param_with_messages(
        &params,
        0,
        "method requires Base64 payload",
        |text| format!("invalid Base64 payload: {text}"),
    )
    .expect_err("invalid base64");

    assert!(
        err.to_string()
            .contains("invalid Base64 payload: not-base64"),
        "{err}"
    );
}
