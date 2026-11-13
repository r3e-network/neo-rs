use neo_primitives::NetworkError;
use std::net::SocketAddr;

#[test]
fn network_error_protocol_violation_display() {
    let peer: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let err = NetworkError::ProtocolViolation {
        peer,
        violation: "Invalid message format".to_string(),
    };

    let display = err.to_string();
    assert!(display.contains("127.0.0.1:8080"));
    assert!(display.contains("Invalid message format"));
}

#[test]
fn network_error_invalid_message_display() {
    let err = NetworkError::InvalidMessage("Bad payload".to_string());
    assert_eq!(err.to_string(), "Invalid message: Bad payload");
}

#[test]
fn network_error_connection_error_display() {
    let err = NetworkError::ConnectionError("Connection refused".to_string());
    assert_eq!(err.to_string(), "Connection error: Connection refused");
}

#[test]
fn network_error_timeout_display() {
    let err = NetworkError::Timeout;
    assert_eq!(err.to_string(), "Network timeout");
}

#[test]
fn network_error_other_display() {
    let err = NetworkError::Other("Unknown error".to_string());
    assert_eq!(err.to_string(), "Network error: Unknown error");
}

#[test]
fn network_error_is_timeout_on_timeout() {
    let err = NetworkError::Timeout;
    assert!(err.is_timeout());
}

#[test]
fn network_error_is_timeout_on_non_timeout() {
    assert!(!NetworkError::InvalidMessage("x".into()).is_timeout());
    assert!(!NetworkError::ConnectionError("x".into()).is_timeout());
    assert!(!NetworkError::Other("x".into()).is_timeout());

    let peer: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let err = NetworkError::ProtocolViolation {
        peer,
        violation: "x".to_string(),
    };
    assert!(!err.is_timeout());
}

#[test]
fn network_error_from_io_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
    let net_err: NetworkError = io_err.into();

    match net_err {
        NetworkError::ConnectionError(msg) => {
            assert!(msg.contains("refused"));
        }
        _ => panic!("Expected ConnectionError variant"),
    }
}

#[test]
fn network_error_from_io_error_preserves_message() {
    let io_err = std::io::Error::new(std::io::ErrorKind::TimedOut, "operation timed out");
    let net_err: NetworkError = io_err.into();

    let NetworkError::ConnectionError(msg) = net_err else {
        panic!("Expected ConnectionError variant");
    };

    assert!(msg.contains("timed out"));
}

#[test]
fn network_error_clone() {
    let err = NetworkError::Timeout;
    let cloned = err.clone();
    assert!(cloned.is_timeout());
}

#[test]
fn network_error_debug_format() {
    let err = NetworkError::Timeout;
    let debug = format!("{:?}", err);
    assert_eq!(debug, "Timeout");

    let err = NetworkError::InvalidMessage("test".to_string());
    let debug = format!("{:?}", err);
    assert!(debug.contains("InvalidMessage"));
    assert!(debug.contains("test"));
}

#[test]
fn network_error_protocol_violation_with_ipv6() {
    let peer: SocketAddr = "[::1]:8080".parse().unwrap();
    let err = NetworkError::ProtocolViolation {
        peer,
        violation: "Bad handshake".to_string(),
    };

    let display = err.to_string();
    assert!(display.contains("[::1]:8080"));
    assert!(display.contains("Bad handshake"));
}

#[test]
fn network_error_empty_strings() {
    let err = NetworkError::InvalidMessage(String::new());
    assert_eq!(err.to_string(), "Invalid message: ");

    let err = NetworkError::ConnectionError(String::new());
    assert_eq!(err.to_string(), "Connection error: ");

    let err = NetworkError::Other(String::new());
    assert_eq!(err.to_string(), "Network error: ");
}

#[test]
fn network_result_ok() {
    let result: Result<i32, NetworkError> = Ok(42);
    assert!(matches!(result, Ok(42)));
}

#[test]
fn network_result_err() {
    let result: Result<i32, NetworkError> = Err(NetworkError::Timeout);
    assert!(matches!(result, Err(NetworkError::Timeout)));
}
