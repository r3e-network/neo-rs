use super::{base58, base64};

#[test]
fn test_base58_encoding() {
    let data = b"hello world";
    let encoded = base58::encode(data);
    let decoded = base58::decode(&encoded).unwrap();

    assert_eq!(data, decoded.as_slice());
}

#[test]
fn base58_check_matches_known_vector() {
    let data = [1, 2, 3];
    let encoded = base58::encode_check(&data);
    assert_eq!(encoded, "3DUz7ncyT");

    let decoded = base58::decode_check(&encoded).unwrap();
    assert_eq!(decoded, data);
}

#[test]
fn base58_check_rejects_short_payload() {
    let err = base58::decode_check("1").unwrap_err().to_string();
    assert!(
        err.contains("too short"),
        "unexpected short payload error: {err}"
    );
}

#[test]
fn base58_check_rejects_invalid_checksum() {
    let err = base58::decode_check("3DUz7ncyU").unwrap_err().to_string();
    assert!(
        err.contains("checksum"),
        "unexpected invalid checksum error: {err}"
    );
}

#[test]
fn base64_standard_round_trips_known_vector() {
    let encoded = base64::encode(&[1, 2, 3, 4]);
    assert_eq!(encoded, "AQIDBA==");

    let decoded = base64::decode_lenient(&encoded).unwrap();
    assert_eq!(decoded, [1, 2, 3, 4]);
}

#[test]
fn base64_standard_decode_ignores_whitespace() {
    let decoded = base64::decode_lenient("A \r Q \t I \n D").unwrap();
    assert_eq!(decoded, [1, 2, 3]);
}

#[test]
fn base64_decode_strict_round_trips_and_rejects_whitespace() {
    // Canonical inputs decode (no whitespace tolerance in this primitive).
    assert_eq!(base64::decode_strict("AQIDBA==").unwrap(), [1, 2, 3, 4]);
    assert_eq!(base64::decode_strict("AQID").unwrap(), [1, 2, 3]);
    assert_eq!(base64::decode_strict("").unwrap(), Vec::<u8>::new());
    // Whitespace and non-alphabet bytes are rejected (the caller strips
    // the whitespace .NET tolerates before calling this).
    assert!(base64::decode_strict("AQ ID").is_err());
    assert!(base64::decode_strict("@@@@").is_err());
    // Missing padding (length not a multiple of 4) is rejected.
    assert!(base64::decode_strict("AQI").is_err());
}

#[test]
fn base64_url_no_pad_round_trips_known_vector() {
    let data = b"Subject=test@example.com&Issuer=https://example.com";
    let encoded = base64::url_encode_no_pad(data);
    assert_eq!(
        encoded,
        "U3ViamVjdD10ZXN0QGV4YW1wbGUuY29tJklzc3Vlcj1odHRwczovL2V4YW1wbGUuY29t"
    );

    let decoded = base64::url_decode_no_pad_lenient(&encoded).unwrap();
    assert_eq!(decoded, data);
}

#[test]
fn url_decode_no_pad_strict_round_trips_and_rejects_whitespace() {
    let data = b"Subject=test@example.com&Issuer=https://example.com";
    let encoded = base64::url_encode_no_pad(data);
    assert_eq!(base64::url_decode_no_pad_strict(&encoded).unwrap(), data);
    // No whitespace tolerance here (the caller strips what .NET ignores).
    assert!(base64::url_decode_no_pad_strict("U3Vi amVjdA").is_err());
    // Standard-alphabet '+'/'/' are not part of the URL-safe alphabet.
    assert!(base64::url_decode_no_pad_strict("ab+/").is_err());
}

#[test]
fn base64_url_decode_ignores_whitespace() {
    let decoded = base64::url_decode_no_pad_lenient("U 3 \t V \n \riamVjdA").unwrap();
    assert_eq!(decoded, b"Subject");
}

#[test]
fn base64_rejects_invalid_input() {
    assert!(base64::decode_lenient("@@@").is_err());
    assert!(base64::url_decode_no_pad_lenient("@@@").is_err());
}
