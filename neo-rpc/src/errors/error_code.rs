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
                    _ => None}
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

    // Neo: unknown entities (-101..-109), matching C# Neo.Plugins.RpcServer.RpcError
    // and the server-side rpc_error.rs constructors.
    /// Unknown block.
    UnknownBlock = -101 => "Unknown block", standard = false;
    /// Unknown contract.
    UnknownContract = -102 => "Unknown contract", standard = false;
    /// Unknown transaction.
    UnknownTransaction = -103 => "Unknown transaction", standard = false;
    /// Unknown storage item.
    UnknownStorageItem = -104 => "Unknown storage item", standard = false;
    /// Unknown script container.
    UnknownScriptContainer = -105 => "Unknown script container", standard = false;
    /// Unknown state root.
    UnknownStateRoot = -106 => "Unknown state root", standard = false;
    /// Unknown session.
    UnknownSession = -107 => "Unknown session", standard = false;
    /// Unknown iterator.
    UnknownIterator = -108 => "Unknown iterator", standard = false;
    /// Unknown height.
    UnknownHeight = -109 => "Unknown height", standard = false;

    // Neo: wallet (-300..-305)
    /// Insufficient funds in the opened wallet.
    InsufficientFundsWallet = -300 => "Insufficient funds in wallet", standard = false;
    /// Wallet fee limit exceeded.
    WalletFeeLimit = -301 => "Wallet fee limit exceeded", standard = false;
    /// No opened wallet.
    NoOpenedWallet = -302 => "No opened wallet", standard = false;
    /// Wallet not found.
    WalletNotFound = -303 => "Wallet not found", standard = false;
    /// Wallet not supported.
    WalletNotSupported = -304 => "Wallet not supported", standard = false;
    /// Unknown account.
    UnknownAccount = -305 => "Unknown account", standard = false;

    // Neo: inventory/verification (-500..-512)
    /// Inventory verification failed.
    VerificationFailed = -500 => "Inventory verification failed", standard = false;
    /// Inventory already exists.
    AlreadyExists = -501 => "Inventory already exists", standard = false;
    /// Memory pool capacity reached.
    OutOfMemory = -502 => "Memory pool capacity reached", standard = false;
    /// Already in the memory pool.
    AlreadyInPool = -503 => "Already in pool", standard = false;
    /// Insufficient network fee.
    InsufficientNetworkFee = -504 => "Insufficient network fee", standard = false;
    /// Policy check failed.
    PolicyFailed = -505 => "Policy check failed", standard = false;
    /// Invalid transaction script.
    InvalidScript = -506 => "Invalid transaction script", standard = false;
    /// Invalid transaction attribute.
    InvalidAttribute = -507 => "Invalid transaction attribute", standard = false;
    /// Invalid signature.
    InvalidSignature = -508 => "Invalid signature", standard = false;
    /// Invalid inventory size.
    InvalidSize = -509 => "Invalid inventory size", standard = false;
    /// Expired transaction.
    ExpiredTransaction = -510 => "Expired transaction", standard = false;
    /// Insufficient funds for the network fee.
    InsufficientFunds = -511 => "Insufficient funds for fee", standard = false;
    /// Invalid contract verification function.
    InvalidContractVerification = -512 => "Invalid contract verification function", standard = false;

    // Neo: server / session / oracle / state (-600..-608)
    /// Access denied.
    AccessDenied = -600 => "Access denied", standard = false;
    /// State iterator sessions disabled.
    DisabledSession = -601 => "State iterator sessions disabled", standard = false;
    /// Oracle service disabled.
    OracleDisabled = -602 => "Oracle service disabled", standard = false;
    /// Oracle request already finished.
    OracleRequestFinished = -603 => "Oracle request already finished", standard = false;
    /// Oracle request not found.
    OracleRequestNotFound = -604 => "Oracle request not found", standard = false;
    /// Not a designated oracle node.
    OracleNotDesignatedNode = -605 => "Not a designated oracle node", standard = false;
    /// Old state not supported.
    UnsupportedState = -606 => "Old state not supported", standard = false;
    /// Invalid state proof.
    InvalidProof = -607 => "Invalid state proof", standard = false;
    /// Contract execution failed.
    ExecutionFailed = -608 => "Contract execution failed", standard = false;
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
#[path = "../tests/errors/error_code.rs"]
mod tests;
