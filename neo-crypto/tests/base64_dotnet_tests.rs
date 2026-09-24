use neo_crypto::Base64;

#[test]
fn dotnet_base64_accepts_only_four_whitespace_characters() {
    for whitespace in [' ', '\t', '\r', '\n'] {
        let input = format!("{whitespace}A{whitespace}Q{whitespace}I{whitespace}D{whitespace}");
        assert_eq!(Base64::decode_dotnet_semantics(&input).unwrap(), [1, 2, 3]);
    }
    for whitespace in ['\u{000b}', '\u{000c}', '\u{0085}', '\u{00a0}', '\u{2003}', '\u{2028}'] {
        assert!(Base64::decode_dotnet_semantics(&format!("A{whitespace}QID")).is_err());
    }
    assert_eq!(Base64::decode_dotnet_semantics(" \r\t\n").unwrap(), Vec::<u8>::new());
}

#[test]
fn dotnet_base64_preserves_padding_requirements() {
    for invalid in ["A", "AQ", "AQ=", "AQ===", "=AQ=", "AQ==A", "AQI", "!!!!", "-_=="] {
        assert!(Base64::decode_dotnet_semantics(invalid).is_err(), "{invalid:?}");
    }
    assert_eq!(Base64::decode_dotnet_semantics("AQ = =").unwrap(), [1]);
    assert_eq!(Base64::decode_dotnet_semantics("AQI=").unwrap(), [1, 2]);
    assert_eq!(Base64::decode_dotnet_semantics("AQID").unwrap(), [1, 2, 3]);
}

#[test]
fn dotnet_base64_accepts_unused_trailing_bits_without_changing_strict_decode() {
    assert_eq!(Base64::decode_dotnet_semantics("AR==").unwrap(), [1]);
    assert_eq!(Base64::decode_dotnet_semantics("AQJ=").unwrap(), [1, 2]);
    assert!(Base64::decode("AR==").is_err());
    assert!(Base64::decode("AQJ=").is_err());
    assert!(Base64::decode("A Q I D").is_err());
}
