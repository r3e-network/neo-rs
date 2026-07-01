use super::*;

#[test]
fn timeout_check_matches_variant() {
    assert!(NetworkError::Timeout.is_timeout());
    assert!(!NetworkError::ConnectionError("x".into()).is_timeout());
    assert!(!NetworkError::InvalidMessage("x".into()).is_timeout());
}
