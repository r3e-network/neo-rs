use super::*;

#[test]
fn retryable_classification() {
    assert!(ServiceError::unavailable("x").is_retryable());
    assert!(ServiceError::timeout("x").is_retryable());
    assert!(!ServiceError::invalid_input("x").is_retryable());
    assert!(!ServiceError::not_found("x").is_retryable());
    assert!(!ServiceError::invalid_state("x").is_retryable());
    assert!(!ServiceError::internal("x").is_retryable());
}

#[test]
fn category_is_stable() {
    assert_eq!(
        ServiceError::unavailable("x").category(),
        "service_unavailable"
    );
    assert_eq!(ServiceError::invalid_input("x").category(), "invalid_input");
    assert_eq!(ServiceError::not_found("x").category(), "not_found");
    assert_eq!(ServiceError::invalid_state("x").category(), "invalid_state");
    assert_eq!(ServiceError::timeout("x").category(), "timeout");
    assert_eq!(ServiceError::internal("x").category(), "internal");
}

#[test]
fn display_includes_message() {
    let err = ServiceError::unavailable("blockchain");
    assert_eq!(err.to_string(), "service unavailable: blockchain");
}
