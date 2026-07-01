//! # neo-native-contracts::tests::std_lib
//!
//! Test module grouping Native StdLib string, memory, and serialization
//! helpers. coverage for neo-native-contracts.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-native-contracts; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - Test modules and fixtures: grouped coverage for the surrounding domain.

use super::*;
use neo_config::Hardfork;
use neo_crypto::Base58;
use neo_primitives::ContractParameterType;
use neo_vm_rs::ExecutionEngineLimits;

fn call(method: &str, arg: &[u8]) -> CoreResult<Vec<u8>> {
    StdLib::dispatch(method, &[arg.to_vec()]).expect("known method")
}

#[test]
fn base64_matches_csharp() {
    // C# StdLib.Base64Encode(utf8("abc")) == "YWJj".
    assert_eq!(call("base64Encode", b"abc").unwrap(), b"YWJj");
    assert_eq!(call("base64Encode", b"").unwrap(), b"");
    assert_eq!(call("base64Encode", &[0xff, 0xfe]).unwrap(), b"//4=");
}

#[test]
fn base64_decode_matches_csharp_vectors() {
    // C# UT_StdLib.TestBinary vectors (the in-repo oracle).
    // Round-trips of Base64Encode output.
    assert_eq!(call("base64Decode", b"").unwrap(), Vec::<u8>::new());
    let enc3 = call("base64Encode", &[1, 2, 3]).unwrap();
    assert_eq!(call("base64Decode", &enc3).unwrap(), vec![1, 2, 3]);
    // Whitespace {space, \r, \t, \n} is stripped before decoding.
    assert_eq!(
        call("base64Decode", b"A \r Q \t I \n D").unwrap(),
        vec![1, 2, 3]
    );
    assert_eq!(call("base64Decode", b"AQIDBA==").unwrap(), vec![1, 2, 3, 4]);
}

#[test]
fn base64_decode_rejects_invalid() {
    // Non-alphabet bytes fault.
    assert!(call("base64Decode", b"@@@@").is_err());
    // Whitespace other than {space, \t, \n, \r} is NOT tolerated (C# faults):
    // a vertical tab (0x0B) survives the strip and faults the strict decode.
    assert!(call("base64Decode", b"AQI\x0bD").is_err());
    // Non-multiple-of-4 length (missing padding) faults.
    assert!(call("base64Decode", b"AQI").is_err());
}

#[test]
fn base64_decode_respects_max_input_length() {
    // 1024 bytes ok ("QQ==" padded chunks stay valid); 1025 faults pre-decode.
    let ok = "A".repeat(MAX_INPUT_LENGTH - 4) + "QQ==";
    assert_eq!(ok.len(), MAX_INPUT_LENGTH);
    assert!(
        StdLib::dispatch("base64Decode", &[ok.into_bytes()])
            .unwrap()
            .is_ok()
    );
    let too_long = "A".repeat(MAX_INPUT_LENGTH + 1);
    assert!(
        StdLib::dispatch("base64Decode", &[too_long.into_bytes()])
            .unwrap()
            .is_err()
    );
}

#[test]
fn base64_encode_respects_max_input_length() {
    let ok = vec![0u8; MAX_INPUT_LENGTH];
    assert!(StdLib::dispatch("base64Encode", &[ok]).unwrap().is_ok());

    let too_long = vec![0u8; MAX_INPUT_LENGTH + 1];
    assert!(
        StdLib::dispatch("base64Encode", &[too_long])
            .unwrap()
            .is_err()
    );
}

#[test]
fn base64_url_matches_csharp_vectors() {
    // C# UT_StdLib.TestBase64Url (the in-repo oracle).
    let plain = "Subject=test@example.com&Issuer=https://example.com";
    let encoded = "U3ViamVjdD10ZXN0QGV4YW1wbGUuY29tJklzc3Vlcj1odHRwczovL2V4YW1wbGUuY29t";
    // base64UrlEncode encodes the UTF-8 bytes of the input string.
    assert_eq!(
        String::from_utf8(call("base64UrlEncode", plain.as_bytes()).unwrap()).unwrap(),
        encoded
    );
    // base64UrlDecode returns the decoded bytes as a string.
    assert_eq!(
        String::from_utf8(call("base64UrlDecode", encoded.as_bytes()).unwrap()).unwrap(),
        plain
    );
    // The four whitespace chars .NET ignores are stripped before decoding.
    let spaced = "U 3 \t V \n \riamVjdD10ZXN0QGV4YW1wbGUuY29tJklzc3Vlcj1odHRwczovL2V4YW1wbGUuY29t";
    assert_eq!(
        String::from_utf8(call("base64UrlDecode", spaced.as_bytes()).unwrap()).unwrap(),
        plain
    );
}

#[test]
fn base64_url_decode_rejects_invalid() {
    // Standard-alphabet '+'/'/' are not URL-safe; a stray vertical tab is not
    // among the tolerated whitespace — both fault.
    assert!(call("base64UrlDecode", b"ab+/").is_err());
    assert!(call("base64UrlDecode", b"U3Vi\x0bamVjdA").is_err());
}

#[test]
fn base64_url_string_edges_match_csharp() {
    // C# parameter binding calls StackItem.GetString(), which uses strict UTF-8.
    assert!(call("base64UrlEncode", &[0xFF]).is_err());

    // Microsoft.IdentityModel.Tokens.Base64UrlEncoder.Decode returns a .NET
    // string via Encoding.UTF8.GetString, whose default fallback replaces
    // malformed bytes with U+FFFD.
    let replacement = call("base64UrlDecode", b"_w").unwrap();
    assert_eq!(
        String::from_utf8(replacement).unwrap(),
        char::REPLACEMENT_CHARACTER.to_string()
    );
}

#[test]
fn base64_url_methods_are_echidna_gated() {
    let c = StdLib::new();
    for name in ["base64UrlEncode", "base64UrlDecode"] {
        let m = c.methods().iter().find(|m| m.name == name).unwrap();
        assert_eq!(
            m.active_in,
            Some(Hardfork::HfEchidna),
            "{name} must gate on Echidna"
        );
    }
}

#[test]
fn hex_encode_decode_matches_csharp_vectors() {
    // C# UT_StdLib.TestHexEncodeDecode: hexEncode([0,1,2,3]) == "00010203".
    assert_eq!(call("hexEncode", &[0, 1, 2, 3]).unwrap(), b"00010203");
    assert_eq!(call("hexDecode", b"00010203").unwrap(), vec![0, 1, 2, 3]);
    assert_eq!(call("hexEncode", b"").unwrap(), b"");
    // Lowercase, no prefix; round-trips arbitrary bytes.
    assert_eq!(call("hexEncode", &[0xab, 0xff]).unwrap(), b"abff");
    assert_eq!(call("hexDecode", b"ABFF").unwrap(), vec![0xab, 0xff]); // case-insensitive
}

#[test]
fn hex_decode_rejects_invalid() {
    // Odd length and non-hex characters fault (Convert.FromHexString parity).
    assert!(call("hexDecode", b"012").is_err());
    assert!(call("hexDecode", b"zz").is_err());
    assert!(call("hexDecode", b"0x10").is_err()); // no "0x" prefix accepted
}

#[test]
fn hex_methods_are_faun_gated() {
    let c = StdLib::new();
    for name in ["hexEncode", "hexDecode"] {
        let m = c.methods().iter().find(|m| m.name == name).unwrap();
        assert_eq!(
            m.active_in,
            Some(Hardfork::HfFaun),
            "{name} must gate on Faun"
        );
    }
}

#[test]
fn base58_round_trips() {
    for sample in [&b"abc"[..], &[0u8, 0, 1, 2, 255][..], &[][..]] {
        let enc = call("base58Encode", sample).unwrap();
        assert_eq!(call("base58Decode", &enc).unwrap(), sample);

        let cenc = call("base58CheckEncode", sample).unwrap();
        assert_eq!(call("base58CheckDecode", &cenc).unwrap(), sample);
    }
    // A corrupted base58check payload must fault.
    assert!(call("base58CheckDecode", b"zzzzzzzz").is_err());
}

#[test]
fn base58_methods_respect_max_input_length() {
    let too_long_bytes = vec![0u8; MAX_INPUT_LENGTH + 1];
    assert!(
        StdLib::dispatch("base58Encode", std::slice::from_ref(&too_long_bytes))
            .unwrap()
            .is_err()
    );
    assert!(
        StdLib::dispatch("base58CheckEncode", std::slice::from_ref(&too_long_bytes))
            .unwrap()
            .is_err()
    );

    let too_long_base58 = "1".repeat(MAX_INPUT_LENGTH + 1);
    assert!(
        StdLib::dispatch("base58Decode", &[too_long_base58.into_bytes()])
            .unwrap()
            .is_err()
    );

    let valid_over_limit_check = Base58::encode_check(&too_long_bytes).into_bytes();
    assert!(valid_over_limit_check.len() > MAX_INPUT_LENGTH);
    assert!(
        StdLib::dispatch("base58CheckDecode", &[valid_over_limit_check])
            .unwrap()
            .is_err()
    );
}

#[test]
fn memory_compare_matches_csharp_sign() {
    let cmp = |a: &[u8], b: &[u8]| -> BigInt {
        let out = StdLib::dispatch("memoryCompare", &[a.to_vec(), b.to_vec()])
            .unwrap()
            .unwrap();
        BigInt::from_signed_bytes_le(&out)
    };
    assert_eq!(cmp(b"abc", b"abc"), BigInt::from(0));
    assert_eq!(cmp(b"abc", b"abd"), BigInt::from(-1));
    assert_eq!(cmp(b"abd", b"abc"), BigInt::from(1));
    // Prefix is "less" than the longer string (SequenceCompareTo semantics).
    assert_eq!(cmp(b"ab", b"abc"), BigInt::from(-1));
    assert_eq!(cmp(b"abc", b"ab"), BigInt::from(1));
}

#[test]
fn memory_compare_respects_max_input_length() {
    let too_long = vec![0u8; MAX_INPUT_LENGTH + 1];
    assert!(
        StdLib::dispatch("memoryCompare", &[too_long.clone(), Vec::new()])
            .unwrap()
            .is_err()
    );
    assert!(
        StdLib::dispatch("memoryCompare", &[Vec::new(), too_long])
            .unwrap()
            .is_err()
    );
}

#[test]
fn unknown_method_is_none() {
    assert!(StdLib::dispatch("notAStdLibMethod", &[vec![1]]).is_none());
}

/// stringSplit via the dispatch seam: decodes the BinarySerialized Array
/// return back into the substrings for comparison.
fn split(args: &[&[u8]]) -> Vec<String> {
    let owned: Vec<Vec<u8>> = args.iter().map(|a| a.to_vec()).collect();
    let bytes = StdLib::dispatch("stringSplit", &owned).unwrap().unwrap();
    let item =
        BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None).unwrap();
    item.as_array()
        .unwrap()
        .iter()
        .map(|s| String::from_utf8(s.as_bytes().unwrap()).unwrap())
        .collect()
}

#[test]
fn string_split_matches_csharp_vector() {
    // C# UT_StdLib.StringSplit: stringSplit("a,b", ",") -> ["a","b"].
    assert_eq!(split(&[b"a,b", b","]), vec!["a", "b"]);
}

#[test]
fn string_split_keeps_empty_entries_by_default() {
    // StringSplitOptions.None keeps empty entries (C# string.Split).
    assert_eq!(split(&[b"a,,b", b","]), vec!["a", "", "b"]);
    assert_eq!(split(&[b",a,", b","]), vec!["", "a", ""]);
    // Empty input -> a single empty element.
    assert_eq!(split(&[b"", b","]), vec![""]);
    // Multi-char separator.
    assert_eq!(split(&[b"a::b::c", b"::"]), vec!["a", "b", "c"]);
    // Empty separator -> the whole string as one element (.NET string overload).
    assert_eq!(split(&[b"abc", b""]), vec!["abc"]);
}

#[test]
fn string_split_remove_empty_entries() {
    // 3-arg overload with removeEmptyEntries = true filters empties.
    assert_eq!(split(&[b"a,,b", b",", &[1]]), vec!["a", "b"]);
    assert_eq!(split(&[b",a,", b",", &[1]]), vec!["a"]);
    assert_eq!(split(&[b"", b",", &[1]]), Vec::<String>::new());
    // removeEmptyEntries = false keeps them (same as the 2-arg form).
    assert_eq!(split(&[b"a,,b", b",", &[0]]), vec!["a", "", "b"]);
}

#[test]
fn string_split_return_encoder_uses_stack_value_projection() {
    let source = include_str!("../../std_lib/mod.rs");
    let start = source
        .find("fn string_split_impl")
        .expect("stringSplit implementation exists");
    let end = source[start..]
        .find("/// C# `StdLib.StrLen")
        .map(|offset| start + offset)
        .expect("next helper marker exists");
    let helper = &source[start..end];

    assert!(helper.contains("StackValue::Array"));
    assert!(helper.contains("serialize_stack_value_default"));
    assert!(!helper.contains("StackItem::from_array"));
    assert!(!helper.contains("BinarySerializer::serialize("));
}

/// itoa via the dispatch seam: `value` is a signed-LE Integer, optional
/// `base` is a signed-LE Integer; the result is the UTF-8 string bytes.
fn itoa(value: i64, base: Option<i64>) -> CoreResult<String> {
    let mut args = vec![BigInt::from(value).to_signed_bytes_le()];
    if let Some(base) = base {
        args.push(BigInt::from(base).to_signed_bytes_le());
    }
    StdLib::dispatch("itoa", &args)
        .unwrap()
        .map(|b| String::from_utf8(b).unwrap())
}

/// atoi via the dispatch seam: `value` is UTF-8 string bytes, optional
/// `base` is a signed-LE Integer; the result is the signed-LE Integer.
fn atoi(value: &str, base: Option<i64>) -> CoreResult<BigInt> {
    let mut args = vec![value.as_bytes().to_vec()];
    if let Some(base) = base {
        args.push(BigInt::from(base).to_signed_bytes_le());
    }
    StdLib::dispatch("atoi", &args)
        .unwrap()
        .map(|b| BigInt::from_signed_bytes_le(&b))
}

#[test]
fn itoa_base10_matches_csharp() {
    // C# Itoa(value) == value.ToString().
    assert_eq!(itoa(0, None).unwrap(), "0");
    assert_eq!(itoa(123, None).unwrap(), "123");
    assert_eq!(itoa(-123, None).unwrap(), "-123");
    assert_eq!(itoa(123, Some(10)).unwrap(), "123");
}

#[test]
fn itoa_base16_matches_dotnet_twos_complement() {
    // C# Itoa(value, 16) == value.ToString("x"): lowercase, sign-disambiguated.
    assert_eq!(itoa(0, Some(16)).unwrap(), "0");
    assert_eq!(itoa(1, Some(16)).unwrap(), "1");
    assert_eq!(itoa(10, Some(16)).unwrap(), "0a"); // top nibble >= 8 -> leading 0
    assert_eq!(itoa(15, Some(16)).unwrap(), "0f");
    assert_eq!(itoa(16, Some(16)).unwrap(), "10");
    assert_eq!(itoa(127, Some(16)).unwrap(), "7f");
    assert_eq!(itoa(128, Some(16)).unwrap(), "080");
    assert_eq!(itoa(255, Some(16)).unwrap(), "0ff");
    assert_eq!(itoa(256, Some(16)).unwrap(), "100");
    // Negatives render in two's complement.
    assert_eq!(itoa(-1, Some(16)).unwrap(), "f");
    assert_eq!(itoa(-16, Some(16)).unwrap(), "f0");
    assert_eq!(itoa(-128, Some(16)).unwrap(), "80");
    assert_eq!(itoa(-129, Some(16)).unwrap(), "f7f");
    assert_eq!(itoa(-256, Some(16)).unwrap(), "f00");
}

#[test]
fn itoa_invalid_base_faults() {
    assert!(itoa(1, Some(2)).is_err());
    assert!(itoa(1, Some(8)).is_err());
}

#[test]
fn atoi_base10_matches_csharp() {
    assert_eq!(atoi("0", None).unwrap(), BigInt::from(0));
    assert_eq!(atoi("123", None).unwrap(), BigInt::from(123));
    assert_eq!(atoi("-123", None).unwrap(), BigInt::from(-123));
    assert_eq!(atoi("+123", None).unwrap(), BigInt::from(123));
    assert_eq!(atoi("-0", None).unwrap(), BigInt::from(0));
    // AllowLeadingSign rejects whitespace, separators, and junk.
    assert!(atoi(" 1", None).is_err());
    assert!(atoi("1 ", None).is_err());
    assert!(atoi("1.0", None).is_err());
    assert!(atoi("", None).is_err());
    assert!(atoi("+", None).is_err());
    assert!(atoi("0x10", None).is_err());
}

#[test]
fn atoi_base16_matches_dotnet_twos_complement() {
    // AllowHexSpecifier: leading nibble >= 8 -> negative.
    assert_eq!(atoi("ff", Some(16)).unwrap(), BigInt::from(-1));
    assert_eq!(atoi("0ff", Some(16)).unwrap(), BigInt::from(255));
    assert_eq!(atoi("f", Some(16)).unwrap(), BigInt::from(-1));
    assert_eq!(atoi("0f", Some(16)).unwrap(), BigInt::from(15));
    assert_eq!(atoi("80", Some(16)).unwrap(), BigInt::from(-128));
    assert_eq!(atoi("080", Some(16)).unwrap(), BigInt::from(128));
    assert_eq!(atoi("7f", Some(16)).unwrap(), BigInt::from(127));
    assert_eq!(atoi("100", Some(16)).unwrap(), BigInt::from(256));
    assert_eq!(atoi("f00", Some(16)).unwrap(), BigInt::from(-256));
    // Case-insensitive; a leading sign is NOT allowed for hex.
    assert_eq!(atoi("FF", Some(16)).unwrap(), BigInt::from(-1));
    assert!(atoi("-1", Some(16)).is_err());
    assert!(atoi("zz", Some(16)).is_err());
}

#[test]
fn itoa_atoi_round_trip_hex() {
    // atoi(itoa(v, 16), 16) == v across the sign boundary.
    for v in [
        -300i64, -256, -129, -128, -1, 0, 1, 127, 128, 255, 256, 65535,
    ] {
        let hex = itoa(v, Some(16)).unwrap();
        assert_eq!(atoi(&hex, Some(16)).unwrap(), BigInt::from(v), "hex={hex}");
    }
}

#[test]
fn atoi_respects_max_input_length() {
    // C# [MaxLength(1024)] on the input: 1024 bytes ok, 1025 faults.
    let ok = "1".repeat(MAX_INPUT_LENGTH);
    assert!(
        StdLib::dispatch("atoi", &[ok.into_bytes()])
            .unwrap()
            .is_ok()
    );
    let too_long = "1".repeat(MAX_INPUT_LENGTH + 1);
    assert!(
        StdLib::dispatch("atoi", &[too_long.into_bytes()])
            .unwrap()
            .is_err()
    );
}

#[test]
fn native_contract_surface() {
    let c = StdLib::new();
    let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "base64Encode",
            "base64Decode",
            "base58Encode",
            "base58Decode",
            "base58CheckEncode",
            "base58CheckDecode",
            "serialize",
            "deserialize",
            "jsonSerialize",
            "jsonDeserialize",
            "memoryCompare",
            "memorySearch",
            "memorySearch",
            "memorySearch",
            "itoa",
            "itoa",
            "atoi",
            "atoi",
            "stringSplit",
            "stringSplit",
            "strLen",
            "base64UrlEncode",
            "base64UrlDecode",
            "hexEncode",
            "hexDecode"
        ]
    );
    assert!(c.methods().iter().all(|m| m.safe));
    // The three memorySearch overloads are distinguished by parameter count.
    let counts: Vec<usize> = c
        .methods()
        .iter()
        .filter(|m| m.name == "memorySearch")
        .map(|m| m.parameters.len())
        .collect();
    assert_eq!(counts, [2, 3, 4]);
}

/// strLen via the dispatch seam: UTF-8 string bytes in, signed-LE Integer out.
fn str_len(arg: &[u8]) -> CoreResult<i64> {
    StdLib::dispatch("strLen", &[arg.to_vec()])
        .unwrap()
        .map(|b| BigInt::from_signed_bytes_le(&b).to_i64().unwrap())
}

#[test]
fn str_len_matches_csharp_ut_vectors() {
    // C# UT_StdLib.StringElementLength: duck emoji, a-tilde and 'a' are all 1.
    assert_eq!(str_len("\u{1F986}".as_bytes()).unwrap(), 1);
    assert_eq!(str_len("\u{00E3}".as_bytes()).unwrap(), 1);
    assert_eq!(str_len(b"a").unwrap(), 1);
    // C# UT_StdLib.TestInvalidUtf8Sequence: (char)0xff is emitted as the
    // UTF-8 encoding of U+00FF (C3 BF) and counts as one element.
    assert_eq!(str_len(&[0xC3, 0xBF]).unwrap(), 1);
    assert_eq!(str_len(&[0xC3, 0xBF, b'a', b'b']).unwrap(), 3);
    // Decomposed a-tilde is also a single element; empty string is 0.
    assert_eq!(str_len("a\u{0303}".as_bytes()).unwrap(), 1);
    assert_eq!(str_len(b"").unwrap(), 0);
    // The .NET-specific divergence: no GB9c, Indic conjuncts stay split.
    assert_eq!(str_len("\u{0915}\u{094D}\u{0915}".as_bytes()).unwrap(), 2);
    // Emoji ZWJ family sequence and a flag are one element each.
    assert_eq!(
        str_len("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}".as_bytes()).unwrap(),
        1
    );
    assert_eq!(str_len("\u{1F1FA}\u{1F1F8}".as_bytes()).unwrap(), 1);
}

#[test]
fn str_len_rejects_invalid_utf8() {
    // C# converts the ByteString with StrictUTF8: invalid UTF-8 faults.
    assert!(str_len(&[0xFF]).is_err());
    assert!(str_len(&[0xC3]).is_err()); // truncated sequence
    assert!(str_len(&[0xED, 0xA0, 0x80]).is_err()); // surrogate encoding
}

#[test]
fn str_len_respects_max_input_length() {
    // C# [MaxLength(1024)] validates the raw StackItem bytes.
    let ok = vec![b'a'; MAX_INPUT_LENGTH];
    assert_eq!(str_len(&ok).unwrap(), 1024);
    let too_long = vec![b'a'; MAX_INPUT_LENGTH + 1];
    assert!(str_len(&too_long).is_err());
    // The cap is on bytes, not characters: 342 three-byte scalars = 1026 bytes.
    let multibyte = "\u{20AC}".repeat(342);
    assert_eq!(multibyte.len(), 1026);
    assert!(str_len(multibyte.as_bytes()).is_err());
}

#[test]
fn str_len_is_ungated_and_safe() {
    // C# StdLib.cs declares StrLen with CpuFee = 1 << 8 and no hardfork.
    let c = StdLib::new();
    let m = c.methods().iter().find(|m| m.name == "strLen").unwrap();
    assert_eq!(m.active_in, None, "strLen must not be hardfork-gated");
    assert!(m.safe);
    assert_eq!(m.cpu_fee, 1 << 8);
    assert_eq!(m.parameters, vec![ContractParameterType::String]);
    assert_eq!(m.return_type, ContractParameterType::Integer);
}

#[test]
fn memory_search_matches_csharp() {
    let search = |args: &[&[u8]]| -> i64 {
        let owned: Vec<Vec<u8>> = args.iter().map(|a| a.to_vec()).collect();
        let out = StdLib::dispatch("memorySearch", &owned).unwrap().unwrap();
        BigInt::from_signed_bytes_le(&out).to_i64().unwrap()
    };
    let n = |v: i64| BigInt::from(v).to_signed_bytes_le();

    // Forward (2-arg): first occurrence, or -1.
    assert_eq!(search(&[b"hello world", b"o"]), 4);
    assert_eq!(search(&[b"hello world", b"world"]), 6);
    assert_eq!(search(&[b"hello", b"z"]), -1);
    // 3-arg: start offset is added back to the in-slice index.
    assert_eq!(search(&[b"hello world", b"o", &n(5)]), 7);
    // 4-arg backward: last occurrence within mem[0..start].
    assert_eq!(search(&[b"hello world", b"o", &n(11), &[1]]), 7);
    assert_eq!(search(&[b"hello world", b"o", &n(5), &[1]]), 4);
}

#[test]
fn memory_search_start_out_of_range_faults() {
    // C# AsSpan(start) throws when start exceeds the length.
    assert!(
        StdLib::dispatch("memorySearch", &[b"abc".to_vec(), b"a".to_vec(), vec![9]])
            .unwrap()
            .is_err()
    );
}

#[test]
fn memory_search_respects_only_mem_max_input_length() {
    let too_long = vec![0u8; MAX_INPUT_LENGTH + 1];
    assert!(
        StdLib::dispatch("memorySearch", &[too_long.clone(), vec![0]])
            .unwrap()
            .is_err()
    );

    let out = StdLib::dispatch("memorySearch", &[b"abc".to_vec(), too_long])
        .unwrap()
        .unwrap();
    assert_eq!(BigInt::from_signed_bytes_le(&out), BigInt::from(-1));
}

#[test]
fn serialize_deserialize_round_trip_and_fault() {
    use neo_vm::StackItem;
    // The serialize arg arrives already BinarySerialized by the engine, so
    // dispatch("serialize") is a passthrough of that payload.
    let payload = BinarySerializer::serialize(
        &StackItem::from_int(BigInt::from(42)),
        &ExecutionEngineLimits::default(),
    )
    .unwrap();
    assert_eq!(
        StdLib::dispatch("serialize", std::slice::from_ref(&payload))
            .unwrap()
            .unwrap(),
        payload
    );
    // deserialize accepts the valid payload (returns it for the Any-return
    // decode) and faults on malformed input.
    assert_eq!(
        StdLib::dispatch("deserialize", std::slice::from_ref(&payload))
            .unwrap()
            .unwrap(),
        payload
    );
    assert!(
        StdLib::dispatch("deserialize", &[vec![0xff, 0xff, 0xff]])
            .unwrap()
            .is_err()
    );
}

#[test]
fn json_serialize_deserialize_match_csharp_vectors() {
    use neo_vm::StackItem;
    let limits = ExecutionEngineLimits::default();
    // The engine BinarySerializes the `Any` arg before dispatch sees it.
    let ser = |item: &StackItem| -> String {
        let payload = BinarySerializer::serialize(item, &limits).unwrap();
        let json = StdLib::dispatch("jsonSerialize", &[payload])
            .unwrap()
            .unwrap();
        String::from_utf8(json).unwrap()
    };
    // C# UT_StdLib.Json_Serialize.
    assert_eq!(ser(&StackItem::from_int(BigInt::from(5))), "5");
    assert_eq!(ser(&StackItem::from_bool(true)), "true");
    assert_eq!(
        ser(&StackItem::from_byte_string(b"test".to_vec())),
        "\"test\""
    );
    assert_eq!(ser(&StackItem::null()), "null");

    // jsonDeserialize returns the StackItem re-encoded as BinarySerializer
    // bytes (for the engine's Any-return decode); compare to the direct
    // encoding of the expected item.
    let de_eq = |json: &str, item: &StackItem| {
        let out = StdLib::dispatch("jsonDeserialize", &[json.as_bytes().to_vec()])
            .unwrap()
            .unwrap();
        assert_eq!(
            out,
            BinarySerializer::serialize(item, &limits).unwrap(),
            "{json}"
        );
    };
    // C# UT_StdLib.Json_Deserialize.
    de_eq("123", &StackItem::from_int(BigInt::from(123)));
    de_eq("null", &StackItem::null());
    // Faults: invalid JSON ("***") and a fractional number ("no decimals").
    assert!(
        StdLib::dispatch("jsonDeserialize", &[b"***".to_vec()])
            .unwrap()
            .is_err()
    );
    assert!(
        StdLib::dispatch("jsonDeserialize", &[b"123.45".to_vec()])
            .unwrap()
            .is_err()
    );

    // Serialize -> deserialize round-trips a structured value.
    let payload = StdLib::dispatch("jsonDeserialize", &[br#"{"k":[1,true,null]}"#.to_vec()])
        .unwrap()
        .unwrap();
    let item = BinarySerializer::deserialize(&payload, &limits, None).unwrap();
    assert_eq!(ser(&item), r#"{"k":[1,true,null]}"#);
}
