use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_core::smart_contract::method_token::MethodToken;
use neo_core::smart_contract::CallFlags;
use neo_primitives::UInt160;

fn serialize_token(token: &MethodToken) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    <MethodToken as Serializable>::serialize(token, &mut writer).expect("serialize token");
    writer.into_bytes()
}

#[test]
fn method_token_roundtrip_matches_csharp() {
    let token = MethodToken {
        call_flags: CallFlags::ALLOW_CALL,
        hash: UInt160::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff01").unwrap(),
        method: "myMethod".to_string(),
        parameters_count: 123,
        has_return_value: true,
    };

    let bytes = serialize_token(&token);
    let mut reader = MemoryReader::new(&bytes);
    let decoded =
        <MethodToken as Serializable>::deserialize(&mut reader).expect("deserialize token");

    assert_eq!(decoded.call_flags, CallFlags::ALLOW_CALL);
    assert_eq!(
        decoded.hash.to_string(),
        "0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff01"
    );
    assert_eq!(decoded.method, "myMethod");
    assert_eq!(decoded.parameters_count, 123);
    assert!(decoded.has_return_value);
}

#[test]
fn method_token_legacy_vec_api_matches_serializable_wire_bytes() {
    let token = MethodToken {
        call_flags: CallFlags::READ_STATES | CallFlags::ALLOW_CALL,
        hash: UInt160::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff01").unwrap(),
        method: "balanceOf".to_string(),
        parameters_count: 2,
        has_return_value: true,
    };

    let serializable_bytes = serialize_token(&token);
    let mut legacy_bytes = Vec::new();
    token.serialize(&mut legacy_bytes);

    assert_eq!(legacy_bytes, serializable_bytes);
    assert_eq!(token.size(), <MethodToken as Serializable>::size(&token));
}

#[test]
fn method_token_legacy_vec_deserialize_accepts_serializable_wire_bytes() {
    let token = MethodToken {
        call_flags: CallFlags::ALL,
        hash: UInt160::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff01").unwrap(),
        method: "transfer".to_string(),
        parameters_count: 4,
        has_return_value: false,
    };

    let bytes = serialize_token(&token);
    let mut reader = bytes.as_slice();
    let decoded = MethodToken::deserialize(&mut reader).expect("legacy deserialize");

    assert_eq!(decoded, token);
    assert!(reader.is_empty());
}

#[test]
fn method_token_deserialize_rejects_invalid_call_flags() {
    let hash = UInt160::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff01").unwrap();
    let mut writer = BinaryWriter::new();
    writer.write_bytes(&hash.as_bytes()).expect("hash");
    writer.write_var_string("myLongMethod").expect("method");
    writer.write_u16(123).expect("params");
    writer.write_bool(true).expect("return");
    writer.write_u8(0xFF).expect("flags");

    let bytes = writer.into_bytes();
    let mut reader = MemoryReader::new(&bytes);
    assert!(<MethodToken as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn method_token_deserialize_rejects_invalid_boolean_byte() {
    let hash = UInt160::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff01").unwrap();
    let mut writer = BinaryWriter::new();
    writer.write_bytes(&hash.as_bytes()).expect("hash");
    writer.write_var_string("myLongMethod").expect("method");
    writer.write_u16(123).expect("params");
    writer.write_u8(0x02).expect("invalid bool");
    writer.write_u8(CallFlags::ALL.bits()).expect("flags");

    let bytes = writer.into_bytes();
    let mut reader = MemoryReader::new(&bytes);
    assert!(<MethodToken as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn method_token_deserialize_rejects_overlong_method_name() {
    let hash = UInt160::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff01").unwrap();
    let mut writer = BinaryWriter::new();
    writer.write_bytes(&hash.as_bytes()).expect("hash");
    writer
        .write_var_string("myLongMethod-123123123123123123123123")
        .expect("method");
    writer.write_u16(123).expect("params");
    writer.write_bool(true).expect("return");
    writer.write_u8(CallFlags::ALL.bits()).expect("flags");

    let bytes = writer.into_bytes();
    let mut reader = MemoryReader::new(&bytes);
    assert!(<MethodToken as Serializable>::deserialize(&mut reader).is_err());
}
