use neo_primitives::CallFlags;

#[test]
fn call_flags_none_is_zero() {
    assert_eq!(CallFlags::NONE.bits(), 0);
}

#[test]
fn call_flags_individual_bits() {
    assert_eq!(CallFlags::READ_STATES.bits(), 0b0000_0001);
    assert_eq!(CallFlags::WRITE_STATES.bits(), 0b0000_0010);
    assert_eq!(CallFlags::ALLOW_CALL.bits(), 0b0000_0100);
    assert_eq!(CallFlags::ALLOW_NOTIFY.bits(), 0b0000_1000);
}

#[test]
fn call_flags_states_combination() {
    let states = CallFlags::STATES;
    assert!(states.contains(CallFlags::READ_STATES));
    assert!(states.contains(CallFlags::WRITE_STATES));
    assert!(!states.contains(CallFlags::ALLOW_CALL));
    assert!(!states.contains(CallFlags::ALLOW_NOTIFY));
}

#[test]
fn call_flags_read_only_combination() {
    let read_only = CallFlags::READ_ONLY;
    assert!(read_only.contains(CallFlags::READ_STATES));
    assert!(!read_only.contains(CallFlags::WRITE_STATES));
    assert!(read_only.contains(CallFlags::ALLOW_CALL));
    assert!(!read_only.contains(CallFlags::ALLOW_NOTIFY));
}

#[test]
fn call_flags_all_combination() {
    let all = CallFlags::ALL;
    assert!(all.contains(CallFlags::READ_STATES));
    assert!(all.contains(CallFlags::WRITE_STATES));
    assert!(all.contains(CallFlags::ALLOW_CALL));
    assert!(all.contains(CallFlags::ALLOW_NOTIFY));
}

#[test]
fn call_flags_union_operations() {
    let flags = CallFlags::READ_STATES | CallFlags::ALLOW_CALL;
    assert!(flags.contains(CallFlags::READ_STATES));
    assert!(flags.contains(CallFlags::ALLOW_CALL));
    assert!(!flags.contains(CallFlags::WRITE_STATES));
}

#[test]
fn call_flags_intersection_operations() {
    let flags1 = CallFlags::READ_STATES | CallFlags::WRITE_STATES;
    let flags2 = CallFlags::READ_STATES | CallFlags::ALLOW_CALL;
    let intersection = flags1 & flags2;
    assert_eq!(intersection, CallFlags::READ_STATES);
}

#[test]
fn call_flags_difference_operations() {
    let flags = CallFlags::ALL;
    let without_write = flags - CallFlags::WRITE_STATES;
    assert!(without_write.contains(CallFlags::READ_STATES));
    assert!(!without_write.contains(CallFlags::WRITE_STATES));
    assert!(without_write.contains(CallFlags::ALLOW_CALL));
    assert!(without_write.contains(CallFlags::ALLOW_NOTIFY));
}

#[test]
fn call_flags_is_empty() {
    assert!(CallFlags::NONE.is_empty());
    assert!(!CallFlags::READ_STATES.is_empty());
}

#[test]
fn call_flags_is_all() {
    assert!(CallFlags::ALL.is_all());
    assert!(!CallFlags::STATES.is_all());
}

#[test]
fn call_flags_default_is_none() {
    let default_flags = CallFlags::default();
    assert_eq!(default_flags, CallFlags::NONE);
}

#[test]
fn call_flags_clone_and_copy() {
    let flags = CallFlags::READ_STATES;
    let cloned = flags.clone();
    let copied = flags;
    assert_eq!(flags, cloned);
    assert_eq!(flags, copied);
}

#[test]
fn call_flags_debug_format() {
    let flags = CallFlags::READ_STATES | CallFlags::WRITE_STATES;
    let debug_str = format!("{:?}", flags);
    assert!(debug_str.contains("READ_STATES"));
    assert!(debug_str.contains("WRITE_STATES"));
}

#[test]
fn call_flags_serialize_to_u8() {
    let flags = CallFlags::READ_STATES | CallFlags::ALLOW_CALL;
    let serialized = serde_json::to_string(&flags).unwrap();
    assert_eq!(serialized, "5"); // 0b0101 = 5
}

#[test]
fn call_flags_deserialize_from_u8() {
    let deserialized: CallFlags = serde_json::from_str("5").unwrap();
    assert_eq!(deserialized, CallFlags::READ_STATES | CallFlags::ALLOW_CALL);
}

#[test]
fn call_flags_deserialize_invalid_bits() {
    let result = serde_json::from_str::<CallFlags>("255");
    assert!(result.is_err());
}

#[test]
fn call_flags_roundtrip_serialization() {
    for flags in [
        CallFlags::NONE,
        CallFlags::READ_STATES,
        CallFlags::WRITE_STATES,
        CallFlags::ALLOW_CALL,
        CallFlags::ALLOW_NOTIFY,
        CallFlags::STATES,
        CallFlags::READ_ONLY,
        CallFlags::ALL,
    ] {
        let serialized = serde_json::to_string(&flags).unwrap();
        let deserialized: CallFlags = serde_json::from_str(&serialized).unwrap();
        assert_eq!(flags, deserialized);
    }
}

#[test]
fn call_flags_equality() {
    assert_eq!(CallFlags::READ_STATES, CallFlags::READ_STATES);
    assert_ne!(CallFlags::READ_STATES, CallFlags::WRITE_STATES);
    assert_eq!(
        CallFlags::READ_STATES | CallFlags::WRITE_STATES,
        CallFlags::STATES
    );
}
