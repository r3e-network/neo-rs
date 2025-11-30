//! JSON-RPC error codes matching Neo RPC server implementation.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Standard JSON-RPC 2.0 error codes and Neo-specific error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(i32)]
pub enum RpcErrorCode {
    // === Standard JSON-RPC 2.0 error codes ===
    /// Invalid JSON was received by the server.
    ParseError = -32700,
    /// The JSON sent is not a valid Request object.
    InvalidRequest = -32600,
    /// The method does not exist / is not available.
    MethodNotFound = -32601,
    /// Invalid method parameter(s).
    InvalidParams = -32602,
    /// Internal JSON-RPC error.
    InternalError = -32603,

    // === Neo-specific error codes ===
    /// Unknown block.
    UnknownBlock = -100,
    /// Unknown contract.
    UnknownContract = -101,
    /// Unknown transaction.
    UnknownTransaction = -102,
    /// Unknown storage item.
    UnknownStorageItem = -103,
    /// Unknown script container.
    UnknownScriptContainer = -104,
    /// Unknown state root.
    UnknownStateRoot = -105,
    /// Unknown session.
    UnknownSession = -106,
    /// Unknown iterator.
    UnknownIterator = -107,
    /// Unknown height.
    UnknownHeight = -108,

    /// Insufficient funds for transfer.
    InsufficientFunds = -300,
    /// Fee limit exceeded.
    WalletFeeLimitExceeded = -301,
    /// No opened wallet.
    NoOpenedWallet = -302,
    /// Invalid wallet password.
    InvalidWalletPassword = -303,

    /// Inventory already exists.
    AlreadyExists = -500,
    /// Memory pool is full.
    MempoolCapReached = -501,
    /// Already in pool.
    AlreadyInPool = -502,
    /// Insufficient network fee.
    InsufficientNetworkFee = -503,
    /// Policy check failed.
    PolicyFailed = -504,
    /// Invalid script.
    InvalidScript = -505,
    /// Invalid attribute.
    InvalidAttribute = -506,
    /// Invalid signature.
    InvalidSignature = -507,
    /// Invalid size.
    InvalidSize = -508,
    /// Expired transaction.
    ExpiredTransaction = -509,
    /// Insufficient funds.
    InsufficientFundsForFee = -510,
    /// Invalid verification script.
    InvalidVerificationScript = -511,

    /// Access denied.
    AccessDenied = -600,

    /// Session not found.
    SessionNotFound = -700,
    /// Oracle not found.
    OracleNotFound = -701,
    /// Oracle request not found.
    OracleRequestNotFound = -702,
}

impl RpcErrorCode {
    /// Returns the numeric error code.
    pub fn code(self) -> i32 {
        self as i32
    }

    /// Creates an error code from a numeric value.
    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            -32700 => Some(Self::ParseError),
            -32600 => Some(Self::InvalidRequest),
            -32601 => Some(Self::MethodNotFound),
            -32602 => Some(Self::InvalidParams),
            -32603 => Some(Self::InternalError),
            -100 => Some(Self::UnknownBlock),
            -101 => Some(Self::UnknownContract),
            -102 => Some(Self::UnknownTransaction),
            -103 => Some(Self::UnknownStorageItem),
            -104 => Some(Self::UnknownScriptContainer),
            -105 => Some(Self::UnknownStateRoot),
            -106 => Some(Self::UnknownSession),
            -107 => Some(Self::UnknownIterator),
            -108 => Some(Self::UnknownHeight),
            -300 => Some(Self::InsufficientFunds),
            -301 => Some(Self::WalletFeeLimitExceeded),
            -302 => Some(Self::NoOpenedWallet),
            -303 => Some(Self::InvalidWalletPassword),
            -500 => Some(Self::AlreadyExists),
            -501 => Some(Self::MempoolCapReached),
            -502 => Some(Self::AlreadyInPool),
            -503 => Some(Self::InsufficientNetworkFee),
            -504 => Some(Self::PolicyFailed),
            -505 => Some(Self::InvalidScript),
            -506 => Some(Self::InvalidAttribute),
            -507 => Some(Self::InvalidSignature),
            -508 => Some(Self::InvalidSize),
            -509 => Some(Self::ExpiredTransaction),
            -510 => Some(Self::InsufficientFundsForFee),
            -511 => Some(Self::InvalidVerificationScript),
            -600 => Some(Self::AccessDenied),
            -700 => Some(Self::SessionNotFound),
            -701 => Some(Self::OracleNotFound),
            -702 => Some(Self::OracleRequestNotFound),
            _ => None,
        }
    }

    /// Returns the default message for this error code.
    pub fn message(self) -> &'static str {
        match self {
            Self::ParseError => "Parse error",
            Self::InvalidRequest => "Invalid request",
            Self::MethodNotFound => "Method not found",
            Self::InvalidParams => "Invalid params",
            Self::InternalError => "Internal error",
            Self::UnknownBlock => "Unknown block",
            Self::UnknownContract => "Unknown contract",
            Self::UnknownTransaction => "Unknown transaction",
            Self::UnknownStorageItem => "Unknown storage item",
            Self::UnknownScriptContainer => "Unknown script container",
            Self::UnknownStateRoot => "Unknown state root",
            Self::UnknownSession => "Unknown session",
            Self::UnknownIterator => "Unknown iterator",
            Self::UnknownHeight => "Unknown height",
            Self::InsufficientFunds => "Insufficient funds",
            Self::WalletFeeLimitExceeded => "Wallet fee limit exceeded",
            Self::NoOpenedWallet => "No opened wallet",
            Self::InvalidWalletPassword => "Invalid wallet password",
            Self::AlreadyExists => "Already exists",
            Self::MempoolCapReached => "Memory pool capacity reached",
            Self::AlreadyInPool => "Already in pool",
            Self::InsufficientNetworkFee => "Insufficient network fee",
            Self::PolicyFailed => "Policy check failed",
            Self::InvalidScript => "Invalid script",
            Self::InvalidAttribute => "Invalid attribute",
            Self::InvalidSignature => "Invalid signature",
            Self::InvalidSize => "Invalid size",
            Self::ExpiredTransaction => "Expired transaction",
            Self::InsufficientFundsForFee => "Insufficient funds for fee",
            Self::InvalidVerificationScript => "Invalid verification script",
            Self::AccessDenied => "Access denied",
            Self::SessionNotFound => "Session not found",
            Self::OracleNotFound => "Oracle not found",
            Self::OracleRequestNotFound => "Oracle request not found",
        }
    }

    /// Returns true if this is a standard JSON-RPC error code.
    pub fn is_standard(self) -> bool {
        matches!(
            self,
            Self::ParseError
                | Self::InvalidRequest
                | Self::MethodNotFound
                | Self::InvalidParams
                | Self::InternalError
        )
    }
}

impl fmt::Display for RpcErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message(), self.code())
    }
}

impl From<RpcErrorCode> for i32 {
    fn from(code: RpcErrorCode) -> Self {
        code.code()
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(RpcErrorCode::UnknownBlock.code(), -100);
        assert_eq!(RpcErrorCode::UnknownContract.code(), -101);
        assert_eq!(RpcErrorCode::InsufficientFunds.code(), -300);
        assert_eq!(RpcErrorCode::AlreadyExists.code(), -500);
        assert_eq!(RpcErrorCode::AccessDenied.code(), -600);
    }

    #[test]
    fn test_from_code() {
        assert_eq!(RpcErrorCode::from_code(-32700), Some(RpcErrorCode::ParseError));
        assert_eq!(RpcErrorCode::from_code(-100), Some(RpcErrorCode::UnknownBlock));
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
}
