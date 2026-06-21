use super::*;

#[test]
fn test_state_root_key() {
    let key = Keys::state_root(12345);
    assert_eq!(key.len(), 5);
    assert_eq!(key[0], 0x01);
    // 12345 in big-endian is 0x00003039
    assert_eq!(&key[1..], &[0x00, 0x00, 0x30, 0x39]);
}

#[test]
fn test_constant_keys() {
    assert_eq!(Keys::CURRENT_LOCAL_ROOT_INDEX, &[0x02]);
    assert_eq!(Keys::CURRENT_VALIDATED_ROOT_INDEX, &[0x04]);
}
