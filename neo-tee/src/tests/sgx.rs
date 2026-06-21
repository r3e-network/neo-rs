use super::*;

#[test]
fn test_report_data_from_user_data_padding() {
    let data = report_data_from_user_data(b"neo");
    assert_eq!(&data[..3], b"neo");
    assert!(data[3..].iter().all(|b| *b == 0));
}

#[test]
fn test_parse_key_file_contents_binary_and_hex() {
    let binary = [0xABu8; 32];
    assert_eq!(parse_key_file_contents(&binary), Some(binary));

    let hex = "ab".repeat(32);
    assert_eq!(parse_key_file_contents(hex.as_bytes()), Some(binary));
}

#[test]
fn test_key_binding_digest_is_deterministic() {
    let key = [0x42u8; 32];
    let digest1 = derive_key_binding_digest(&key);
    let digest2 = derive_key_binding_digest(&key);
    assert_eq!(digest1, digest2);
}
