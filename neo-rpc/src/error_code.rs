//! JSON-RPC error codes matching Neo RPC server implementation.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! rpc_error_codes {
    (
        $(
            $(#[$meta:meta])*
            $variant:ident = $code:expr_2021 => $message:expr_2021, standard = $standard:expr_2021;
        )+
    ) => {
        /// Standard JSON-RPC 2.0 error codes and Neo-specific error codes.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[repr(i32)]
        pub enum RpcErrorCode {
            $(
                $(#[$meta])*
                $variant = $code,
            )+
        }

        impl RpcErrorCode {
            /// Returns the numeric error code.
            #[must_use]
            pub const fn code(self) -> i32 {
                self as i32
            }

            /// Creates an error code from a numeric value.
            #[must_use]
            pub const fn from_code(code: i32) -> Option<Self> {
                match code {
                    $(
                        $code => Some(Self::$variant),
                    )+
                    _ => None,
                }
            }

            /// Returns the default message for this error code.
            #[must_use]
            pub const fn message(self) -> &'static str {
                match self {
                    $(
                        Self::$variant => $message,
                    )+
                }
            }

            /// Returns true if this is a standard JSON-RPC error code.
            #[must_use]
            pub const fn is_standard(self) -> bool {
                match self {
                    $(
                        Self::$variant => $standard,
                    )+
                }
            }
        }
    };
}

rpc_error_codes! {
    /// Invalid JSON was received by the server.
    ParseError = -32700 => "Parse error", standard = true;
    /// The JSON sent is not a valid Request object.
    InvalidRequest = -32600 => "Invalid request", standard = true;
    /// The method does not exist / is not available.
    MethodNotFound = -32601 => "Method not found", standard = true;
    /// Invalid method parameter(s).
    InvalidParams = -32602 => "Invalid params", standard = true;
    /// Internal JSON-RPC error.
    InternalError = -32603 => "Internal error", standard = true;

    /// Unknown block.
    UnknownBlock = -100 => "Unknown block", standard = false;
    /// Unknown contract.
    UnknownContract = -101 => "Unknown contract", standard = false;
    /// Unknown transaction.
    UnknownTransaction = -102 => "Unknown transaction", standard = false;
    /// Unknown storage item.
    UnknownStorageItem = -103 => "Unknown storage item", standard = false;
    /// Unknown script container.
    UnknownScriptContainer = -104 => "Unknown script container", standard = false;
    /// Unknown state root.
    UnknownStateRoot = -105 => "Unknown state root", standard = false;
    /// Unknown session.
    UnknownSession = -106 => "Unknown session", standard = false;
    /// Unknown iterator.
    UnknownIterator = -107 => "Unknown iterator", standard = false;
    /// Unknown height.
    UnknownHeight = -108 => "Unknown height", standard = false;

    /// Insufficient funds for transfer.
    InsufficientFunds = -300 => "Insufficient funds", standard = false;
    /// Fee limit exceeded.
    WalletFeeLimitExceeded = -301 => "Wallet fee limit exceeded", standard = false;
    /// No opened wallet.
    NoOpenedWallet = -302 => "No opened wallet", standard = false;
    /// Invalid wallet password.
    InvalidWalletPassword = -303 => "Invalid wallet password", standard = false;

    /// Inventory already exists.
    AlreadyExists = -500 => "Already exists", standard = false;
    /// Memory pool is full.
    MempoolCapReached = -501 => "Memory pool capacity reached", standard = false;
    /// Already in pool.
    AlreadyInPool = -502 => "Already in pool", standard = false;
    /// Insufficient network fee.
    InsufficientNetworkFee = -503 => "Insufficient network fee", standard = false;
    /// Policy check failed.
    PolicyFailed = -504 => "Policy check failed", standard = false;
    /// Invalid script.
    InvalidScript = -505 => "Invalid script", standard = false;
    /// Invalid attribute.
    InvalidAttribute = -506 => "Invalid attribute", standard = false;
    /// Invalid signature.
    InvalidSignature = -507 => "Invalid signature", standard = false;
    /// Invalid size.
    InvalidSize = -508 => "Invalid size", standard = false;
    /// Expired transaction.
    ExpiredTransaction = -509 => "Expired transaction", standard = false;
    /// Insufficient funds.
    InsufficientFundsForFee = -510 => "Insufficient funds for fee", standard = false;
    /// Invalid verification script.
    InvalidVerificationScript = -511 => "Invalid verification script", standard = false;

    /// Access denied.
    AccessDenied = -600 => "Access denied", standard = false;

    /// Session not found.
    SessionNotFound = -700 => "Session not found", standard = false;
    /// Oracle not found.
    OracleNotFound = -701 => "Oracle not found", standard = false;
    /// Oracle request not found.
    OracleRequestNotFound = -702 => "Oracle request not found", standard = false;
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
        assert_eq!(
            RpcErrorCode::from_code(-32700),
            Some(RpcErrorCode::ParseError)
        );
        assert_eq!(
            RpcErrorCode::from_code(-100),
            Some(RpcErrorCode::UnknownBlock)
        );
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
