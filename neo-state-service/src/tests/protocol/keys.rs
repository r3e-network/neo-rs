use super::*;

#[test]
fn test_state_root_key() {
    let key = Keys::state_root(12345);
    assert_eq!(key.len(), STATE_ROOT_KEY_LEN);
    assert_eq!(key[0], STATE_ROOT_PREFIX);
    // 12345 in big-endian is 0x00003039
    assert_eq!(&key[1..], &[0x00, 0x00, 0x30, 0x39]);
    assert_eq!(state_root_index(&key), Some(12345));
}

#[test]
fn state_root_index_rejects_wrong_namespace_and_length() {
    assert_eq!(state_root_index(&[]), None);
    assert_eq!(state_root_index(&[STATE_ROOT_PREFIX]), None);
    assert_eq!(state_root_index(&[0x02, 0, 0, 0, 1]), None);
    assert_eq!(state_root_index(&[STATE_ROOT_PREFIX, 0, 0, 0, 1, 0]), None);
}

#[test]
fn test_constant_keys() {
    assert_eq!(Keys::CURRENT_LOCAL_ROOT_INDEX, &[0x02]);
    assert_eq!(Keys::CURRENT_VALIDATED_ROOT_INDEX, &[0x04]);
}
