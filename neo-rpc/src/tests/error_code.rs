use super::*;

#[test]
fn test_standard_error_codes() {
    assert_eq!(RpcErrorCode::ParseError.code(), -32700);
    assert_eq!(RpcErrorCode::InvalidRequest.code(), -32600);
    assert_eq!(RpcErrorCode::MethodNotFound.code(), -32601);
    assert_eq!(RpcErrorCode::InvalidParams.code(), -32602);
    assert_eq!(RpcErrorCode::InternalError.code(), -32603);
}

#[test]
fn test_neo_error_codes() {
    // Codes match C# Neo.Plugins.RpcServer.RpcError.
    assert_eq!(RpcErrorCode::UnknownBlock.code(), -101);
    assert_eq!(RpcErrorCode::UnknownContract.code(), -102);
    assert_eq!(RpcErrorCode::UnknownHeight.code(), -109);
    assert_eq!(RpcErrorCode::InsufficientFundsWallet.code(), -300);
    assert_eq!(RpcErrorCode::VerificationFailed.code(), -500);
    assert_eq!(RpcErrorCode::AlreadyExists.code(), -501);
    assert_eq!(RpcErrorCode::InsufficientFunds.code(), -511);
    assert_eq!(RpcErrorCode::AccessDenied.code(), -600);
    assert_eq!(RpcErrorCode::ExecutionFailed.code(), -608);
}

#[test]
fn test_from_code() {
    assert_eq!(
        RpcErrorCode::from_code(-32700),
        Some(RpcErrorCode::ParseError)
    );
    assert_eq!(
        RpcErrorCode::from_code(-101),
        Some(RpcErrorCode::UnknownBlock)
    );
    // -100 is no longer a valid code (the group starts at -101).
    assert_eq!(RpcErrorCode::from_code(-100), None);
    assert_eq!(RpcErrorCode::from_code(-999), None);
}

#[test]
fn test_is_standard() {
    assert!(RpcErrorCode::ParseError.is_standard());
    assert!(RpcErrorCode::MethodNotFound.is_standard());
    assert!(!RpcErrorCode::UnknownBlock.is_standard());
    assert!(!RpcErrorCode::AccessDenied.is_standard());
}

#[test]
fn test_message() {
    assert_eq!(RpcErrorCode::ParseError.message(), "Parse error");
    assert_eq!(RpcErrorCode::UnknownBlock.message(), "Unknown block");
}

#[test]
fn test_display() {
    let code = RpcErrorCode::MethodNotFound;
    assert_eq!(code.to_string(), "Method not found (-32601)");
}
