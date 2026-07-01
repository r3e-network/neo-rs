//! # neo-rpc::server::rpc_error
//!
//! RPC error records exposed by the server boundary.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `tests`: Module-local tests and regression coverage.

// This module mirrors Neo.Plugins.RpcServer.RpcError from the C# codebase while
// following idiomatic Rust patterns. It provides strongly-typed error instances
// that can be serialised to JSON responses for the RPC subsystem.

use neo_primitives::UInt160;
use neo_serialization::json::{JObject, JToken};
use std::fmt::{self, Display};

/// Represents a JSON-RPC error returned by the RPC server (matches the C#
/// `RpcError` class semantics).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RpcError {
    code: i32,
    message: String,
    data: Option<String>,
}

macro_rules! rpc_error_constructors {
    (
        $(
            $(#[$meta:meta])*
            $name:ident => ($code:expr_2021, $message:expr_2021 $(, data = $data:expr_2021)?);
        )+
    ) => {
        $(
            $(#[$meta])*
            #[must_use]
            pub fn $name() -> Self {
                Self::new($code, $message, rpc_error_constructors!(@data $($data)?))
           }
        )+
   };

    (@data) => {
        None
   };

    (@data $data:expr_2021) => {
        Some($data.to_string())
   };
}

impl RpcError {
    /// Creates a new `RpcError` instance.
    pub fn new(code: i32, message: impl Into<String>, data: Option<String>) -> Self {
        let message = message.into();
        let data = data.and_then(|value| {
            let trimmed = value.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        Self {
            code,
            message,
            data,
        }
    }

    /// Returns the JSON-RPC error code.
    #[must_use]
    pub const fn code(&self) -> i32 {
        self.code
    }

    /// Returns the human readable error message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns any additional error data when available.
    #[must_use]
    pub fn data(&self) -> Option<&str> {
        self.data.as_deref()
    }

    /// Creates a copy of the error carrying an additional data payload.
    pub fn with_data(&self, data: impl Into<String>) -> Self {
        Self {
            code: self.code,
            message: self.message.clone(),
            data: {
                let value = data.into();
                if value.trim().is_empty() {
                    None
                } else {
                    Some(value)
                }
            },
        }
    }

    /// Returns the formatted error message used for exceptions/logging.
    #[must_use]
    pub fn error_message(&self) -> String {
        match &self.data {
            Some(data) => format!("{} - {}", self.message, data),
            None => self.message.clone(),
        }
    }

    /// Serialises the error into a Neo JSON token (matches C# `ToJson`).
    #[must_use]
    pub fn to_json(&self) -> JToken {
        let mut obj = JObject::new();
        obj.set(
            "code".to_string(),
            Some(JToken::Number(f64::from(self.code))),
        );
        obj.set(
            "message".to_string(),
            Some(JToken::String(self.error_message())),
        );
        if let Some(data) = &self.data {
            obj.set("data".to_string(), Some(JToken::String(data.clone())));
        }
        JToken::Object(obj)
    }

    /// Error for a contract that has no compatible `verify` method.
    pub fn invalid_contract_verification_hash(contract_hash: &UInt160, pcount: i32) -> RpcError {
        RpcError::invalid_contract_verification().with_data(format!(
            "The smart contract {contract_hash} haven't got verify method with {pcount} input parameters."
        ))
    }

    rpc_error_constructors! {
         /// Invalid JSON-RPC request (spec defined).
         invalid_request => (-32600, "Invalid request");
         /// Unknown RPC method.
         method_not_found => (-32601, "Method not found");
         /// Invalid method parameters.
         invalid_params => (-32602, "Invalid params");
         /// Internal JSON-RPC error.
         internal_server_error => (-32603, "Internal server RpcError");
         /// Server-side rate limiting triggered.
         ///
         /// Uses the JSON-RPC server error range (-32000..-32099).
         too_many_requests => (-32001, "Too many requests");
         /// Malformed JSON payload.
         bad_request => (-32700, "Bad request");
         /// Unknown block referenced in the request.
         unknown_block => (-101, "Unknown block");
         /// Unknown contract referenced in the request.
         unknown_contract => (-102, "Unknown contract");
         /// Unknown transaction referenced in the request.
         unknown_transaction => (-103, "Unknown transaction");
         /// Unknown storage item referenced in the request.
         unknown_storage_item => (-104, "Unknown storage item");
         /// Unknown script container referenced in the request.
         unknown_script_container => (-105, "Unknown script container");
         /// Unknown state root referenced in the request.
         unknown_state_root => (-106, "Unknown state root");
         /// Unknown iterator identifier.
         unknown_iterator => (-108, "Unknown iterator");
         /// Unknown iterator session identifier.
         unknown_session => (-107, "Unknown session");
         /// Unknown block height.
         unknown_height => (-109, "Unknown height");
         /// Insufficient funds inside a wallet context.
         insufficient_funds_wallet => (-300, "Insufficient funds in wallet");
         /// Wallet fee limit exceeded.
         wallet_fee_limit => (
             -301,
             "Wallet fee limit exceeded",
             data = "The necessary fee is more than the MaxFee, this transaction is failed. Please increase your MaxFee value."
         );
         /// No wallet opened.
         no_opened_wallet => (-302, "No opened wallet");
         /// Wallet not found.
         wallet_not_found => (-303, "Wallet not found");
         /// Wallet type not supported.
         wallet_not_supported => (-304, "Wallet not supported");
         /// Unknown account referenced in request.
         unknown_account => (-305, "Unknown account");
         /// Inventory verification failed.
         verification_failed => (-500, "Inventory verification failed");
         /// Inventory already exists.
         already_exists => (-501, "Inventory already exists");
         /// Mempool capacity reached.
         mempool_cap_reached => (-502, "Memory pool capacity reached");
         /// Inventory already present in pool.
         already_in_pool => (-503, "Already in pool");
         /// Insufficient network fee supplied.
         insufficient_network_fee => (-504, "Insufficient network fee");
         /// Policy check failed.
         policy_failed => (-505, "Policy check failed");
         /// Transaction script invalid.
         invalid_script => (-506, "Invalid transaction script");
         /// Invalid transaction attribute.
         invalid_attribute => (-507, "Invalid transaction attribute");
         /// Invalid signature detected.
         invalid_signature => (-508, "Invalid signature");
         /// Inventory payload size invalid.
         invalid_size => (-509, "Invalid inventory size");
         /// Transaction expired.
         expired_transaction => (-510, "Expired transaction");
         /// Insufficient funds to cover fees.
         insufficient_funds => (-511, "Insufficient funds for fee");
         /// Contract verification routine invalid.
         invalid_contract_verification => (-512, "Invalid contract verification function");
         /// Access denied for the requested operation.
         access_denied => (-600, "Access denied");
         /// Iterator session feature disabled.
         sessions_disabled => (-601, "State iterator sessions disabled");
         /// Oracle service disabled.
         oracle_disabled => (-602, "Oracle service disabled");
         /// Oracle request already finished.
         oracle_request_finished => (-603, "Oracle request already finished");
         /// Oracle request not found.
         oracle_request_not_found => (-604, "Oracle request not found");
         /// Node is not designated oracle node.
         oracle_not_designated_node => (-605, "Not a designated oracle node");
         /// Requested state is not supported (old state).
         unsupported_state => (-606, "Old state not supported");
         /// Invalid state proof supplied.
         invalid_proof => (-607, "Invalid state proof");
         /// Contract execution failed.
         execution_failed => (-608, "Contract execution failed");
    }
}

impl Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.data {
            Some(data) => write!(f, "{} ({}) - {}", self.message, self.code, data),
            None => write!(f, "{} ({})", self.message, self.code),
        }
    }
}

impl std::error::Error for RpcError {}

impl From<RpcError> for JToken {
    fn from(error: RpcError) -> Self {
        error.to_json()
    }
}

#[cfg(test)]
#[path = "../../tests/server/core/rpc_error.rs"]
mod tests;
