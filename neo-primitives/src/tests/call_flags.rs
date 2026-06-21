use super::*;

/// CallFlags bit values are consensus-observable (they gate CALLT / syscall
/// permissions and appear in contract manifests). They must match C# exactly.
#[test]
fn call_flags_bit_values_match_csharp() {
    assert_eq!(CallFlags::NONE.bits(), 0x00);
    assert_eq!(CallFlags::READ_STATES.bits(), 0x01);
    assert_eq!(CallFlags::WRITE_STATES.bits(), 0x02);
    assert_eq!(CallFlags::ALLOW_CALL.bits(), 0x04);
    assert_eq!(CallFlags::ALLOW_NOTIFY.bits(), 0x08);
    assert_eq!(CallFlags::STATES.bits(), 0x03);
    assert_eq!(CallFlags::READ_ONLY.bits(), 0x05);
    assert_eq!(CallFlags::ALL.bits(), 0x0F);
}

#[test]
fn call_flags_serde_roundtrips_as_u8() {
    let flags = CallFlags::READ_STATES | CallFlags::ALLOW_CALL;
    let json = serde_json::to_string(&flags).unwrap();
    assert_eq!(json, "5");
    let parsed: CallFlags = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, flags);
    assert!(serde_json::from_str::<CallFlags>("16").is_err());
}
