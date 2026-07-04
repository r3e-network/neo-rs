use super::*;

#[test]
fn strip_hex_prefix_handles_both_cases() {
    assert_eq!(strip_hex_prefix("0xdeadbeef"), "deadbeef");
    assert_eq!(strip_hex_prefix("0XDEADBEEF"), "DEADBEEF");
    assert_eq!(strip_hex_prefix("deadbeef"), "deadbeef");
    assert_eq!(strip_hex_prefix("0x"), "");
    assert_eq!(strip_hex_prefix(""), "");
}

#[test]
fn encode_hex_produces_lowercase() {
    assert_eq!(encode_hex(&[0xDE, 0xAD, 0xBE, 0xEF]), "deadbeef");
    assert_eq!(encode_hex(&[]), "");
    assert_eq!(encode_hex(&[0x00]), "00");
}

#[test]
fn decode_hex_strips_prefix() {
    assert_eq!(decode_hex("deadbeef").unwrap(), vec![0xDE, 0xAD, 0xBE, 0xEF]);
    assert_eq!(decode_hex("0xdeadbeef").unwrap(), vec![0xDE, 0xAD, 0xBE, 0xEF]);
    assert_eq!(decode_hex("0XDEADBEEF").unwrap(), vec![0xDE, 0xAD, 0xBE, 0xEF]);
    assert_eq!(decode_hex("").unwrap(), Vec::<u8>::new());
}

#[test]
fn decode_hex_rejects_invalid() {
    assert!(decode_hex("xyz").is_err());
    assert!(decode_hex("0xdeadbee").is_err()); // odd length
}

#[test]
fn encode_reversed_hex_reverses_and_prefixes() {
    // [0xDE, 0xAD, 0xBE, 0xEF] -> reverse -> [0xEF, 0xBE, 0xAD, 0xDE] -> "0xefbeadde"
    assert_eq!(encode_reversed_hex(&[0xDE, 0xAD, 0xBE, 0xEF]), "0xefbeadde");
    assert_eq!(encode_reversed_hex(&[]), "0x");
}

#[test]
fn decode_reversed_hex_reverses_after_decode() {
    // "0xefbeadde" -> strip prefix -> decode -> [0xEF, 0xBE, 0xAD, 0xDE] -> reverse -> [0xDE, 0xAD, 0xBE, 0xEF]
    assert_eq!(decode_reversed_hex("0xefbeadde").unwrap(), vec![0xDE, 0xAD, 0xBE, 0xEF]);
    assert_eq!(decode_reversed_hex("efbeadde").unwrap(), vec![0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn encode_hex_upper_produces_uppercase() {
    assert_eq!(encode_hex_upper(&[0xDE, 0xAD]), "DEAD");
}

#[test]
fn encode_decode_roundtrip() {
    let data = vec![0x00, 0x01, 0x02, 0xFF, 0xAA, 0x55];
    let encoded = encode_hex(&data);
    let decoded = decode_hex(&encoded).unwrap();
    assert_eq!(decoded, data);
}

#[test]
fn encode_decode_reversed_roundtrip() {
    let data = vec![0x00, 0x01, 0x02, 0xFF, 0xAA, 0x55];
    let encoded = encode_reversed_hex(&data);
    let decoded = decode_reversed_hex(&encoded).unwrap();
    assert_eq!(decoded, data);
}
