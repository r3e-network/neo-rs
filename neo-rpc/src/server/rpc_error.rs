// Copyright (C) 2015-2025 The Neo Project.
//
// This module mirrors Neo.Plugins.RpcServer.RpcError from the C# codebase while
// following idiomatic Rust patterns. It provides strongly-typed error instances
// that can be serialised to JSON responses for the RPC subsystem.

use neo_json::{JObject, JToken};
use std::fmt::{self, Display};

/// Represents a JSON-RPC error returned by the RPC server (matches the C#
/// `RpcError` class semantics).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RpcError {
    code: i32,
    message: String,
    data: Option<String>,
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
    pub fn code(&self) -> i32 {
        self.code
    }

    /// Returns the human readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns any additional error data when available.
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
    pub fn error_message(&self) -> String {
        match &self.data {
            Some(data) => format!("{} - {}", self.message, data),
            None => self.message.clone(),
        }
    }

    /// Serialises the error into a Neo JSON token (matches C# `ToJson`).
    pub fn to_json(&self) -> JToken {
        let mut obj = JObject::new();
        obj.set("code".to_string(), Some(JToken::Number(self.code as f64)));
        obj.set(
            "message".to_string(),
            Some(JToken::String(self.error_message())),
        );
        if let Some(data) = &self.data {
            obj.set("data".to_string(), Some(JToken::String(data.clone())));
        }
        JToken::Object(obj)
    }

    /// Utility for errors that simply wrap a message with no data.
    fn simple(code: i32, message: &'static str) -> Self {
        Self::new(code, message, None)
    }

    /// Invalid JSON-RPC request (spec defined).
    pub fn invalid_request() -> Self {
        Self::simple(-32600, "Invalid request")
    }

    /// Unknown RPC method.
    pub fn method_not_found() -> Self {
        Self::simple(-32601, "Method not found")
    }

    /// Invalid method parameters.
    pub fn invalid_params() -> Self {
        Self::simple(-32602, "Invalid params")
    }

    /// Internal JSON-RPC error.
    pub fn internal_server_error() -> Self {
        Self::simple(-32603, "Internal server RpcError")
    }

    /// Server-side rate limiting triggered.
    ///
    /// Uses the JSON-RPC server error range (-32000..-32099).
    pub fn too_many_requests() -> Self {
        Self::simple(-32001, "Too many requests")
    }

    /// Malformed JSON payload.
    pub fn bad_request() -> Self {
        Self::simple(-32700, "Bad request")
    }

    /// Unknown block referenced in the request.
    pub fn unknown_block() -> Self {
        Self::simple(-101, "Unknown block")
    }

    /// Unknown contract referenced in the request.
    pub fn unknown_contract() -> Self {
        Self::simple(-102, "Unknown contract")
    }

    /// Unknown transaction referenced in the request.
    pub fn unknown_transaction() -> Self {
        Self::simple(-103, "Unknown transaction")
    }

    /// Unknown storage item referenced in the request.
    pub fn unknown_storage_item() -> Self {
        Self::simple(-104, "Unknown storage item")
    }

    /// Unknown script container referenced in the request.
    pub fn unknown_script_container() -> Self {
        Self::simple(-105, "Unknown script container")
    }

    /// Unknown state root referenced in the request.
    pub fn unknown_state_root() -> Self {
        Self::simple(-106, "Unknown state root")
    }

    /// Unknown iterator identifier.
    pub fn unknown_iterator() -> Self {
        Self::simple(-108, "Unknown iterator")
    }

    /// Unknown iterator session identifier.
    pub fn unknown_session() -> Self {
        Self::simple(-107, "Unknown session")
    }

    /// Unknown block height.
    pub fn unknown_height() -> Self {
        Self::simple(-109, "Unknown height")
    }

    /// Insufficient funds inside a wallet context.
    pub fn insufficient_funds_wallet() -> Self {
        Self::simple(-300, "Insufficient funds in wallet")
    }

    /// Wallet fee limit exceeded.
    pub fn wallet_fee_limit() -> Self {
        Self::new(
            -301,
            "Wallet fee limit exceeded",
            Some("The necessary fee is more than the MaxFee, this transaction is failed. Please increase your MaxFee value.".to_string()),
        )
    }

    /// No wallet opened.
    pub fn no_opened_wallet() -> Self {
        Self::simple(-302, "No opened wallet")
    }

    /// Wallet not found.
    pub fn wallet_not_found() -> Self {
        Self::simple(-303, "Wallet not found")
    }

    /// Wallet type not supported.
    pub fn wallet_not_supported() -> Self {
        Self::simple(-304, "Wallet not supported")
    }

    /// Unknown account referenced in request.
    pub fn unknown_account() -> Self {
        Self::simple(-305, "Unknown account")
    }

    /// Inventory verification failed.
    pub fn verification_failed() -> Self {
        Self::simple(-500, "Inventory verification failed")
    }

    /// Inventory already exists.
    pub fn already_exists() -> Self {
        Self::simple(-501, "Inventory already exists")
    }

    /// Mempool capacity reached.
    pub fn mempool_cap_reached() -> Self {
        Self::simple(-502, "Memory pool capacity reached")
    }

    /// Inventory already present in pool.
    pub fn already_in_pool() -> Self {
        Self::simple(-503, "Already in pool")
    }

    /// Insufficient network fee supplied.
    pub fn insufficient_network_fee() -> Self {
        Self::simple(-504, "Insufficient network fee")
    }

    /// Policy check failed.
    pub fn policy_failed() -> Self {
        Self::simple(-505, "Policy check failed")
    }

    /// Transaction script invalid.
    pub fn invalid_script() -> Self {
        Self::simple(-506, "Invalid transaction script")
    }

    /// Invalid transaction attribute.
    pub fn invalid_attribute() -> Self {
        Self::simple(-507, "Invalid transaction attribute")
    }

    /// Invalid signature detected.
    pub fn invalid_signature() -> Self {
        Self::simple(-508, "Invalid signature")
    }

    /// Inventory payload size invalid.
    pub fn invalid_size() -> Self {
        Self::simple(-509, "Invalid inventory size")
    }

    /// Transaction expired.
    pub fn expired_transaction() -> Self {
        Self::simple(-510, "Expired transaction")
    }

    /// Insufficient funds to cover fees.
    pub fn insufficient_funds() -> Self {
        Self::simple(-511, "Insufficient funds for fee")
    }

    /// Contract verification routine invalid.
    pub fn invalid_contract_verification() -> Self {
        Self::simple(-512, "Invalid contract verification function")
    }

    /// Access denied for the requested operation.
    pub fn access_denied() -> Self {
        Self::simple(-600, "Access denied")
    }

    /// Iterator session feature disabled.
    pub fn sessions_disabled() -> Self {
        Self::simple(-601, "State iterator sessions disabled")
    }

    /// Oracle service disabled.
    pub fn oracle_disabled() -> Self {
        Self::simple(-602, "Oracle service disabled")
    }

    /// Oracle request already finished.
    pub fn oracle_request_finished() -> Self {
        Self::simple(-603, "Oracle request already finished")
    }

    /// Oracle request not found.
    pub fn oracle_request_not_found() -> Self {
        Self::simple(-604, "Oracle request not found")
    }

    /// Node is not designated oracle node.
    pub fn oracle_not_designated_node() -> Self {
        Self::simple(-605, "Not a designated oracle node")
    }

    /// Requested state is not supported (old state).
    pub fn unsupported_state() -> Self {
        Self::simple(-606, "Old state not supported")
    }

    /// Invalid state proof supplied.
    pub fn invalid_proof() -> Self {
        Self::simple(-607, "Invalid state proof")
    }

    /// Contract execution failed.
    pub fn execution_failed() -> Self {
        Self::simple(-608, "Contract execution failed")
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
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn rpc_error_access_denied_json() {
        let json = RpcError::access_denied().to_json().to_string();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse json");
        assert_eq!(parsed.get("code").and_then(|v| v.as_f64()), Some(-600.0));
        assert_eq!(
            parsed.get("message").and_then(|v| v.as_str()),
            Some("Access denied")
        );
    }

    #[test]
    fn rpc_error_data_only_on_wallet_fee_limit() {
        let errors = vec![
            RpcError::invalid_request(),
            RpcError::method_not_found(),
            RpcError::invalid_params(),
            RpcError::internal_server_error(),
            RpcError::too_many_requests(),
            RpcError::bad_request(),
            RpcError::unknown_block(),
            RpcError::unknown_contract(),
            RpcError::unknown_transaction(),
            RpcError::unknown_storage_item(),
            RpcError::unknown_script_container(),
            RpcError::unknown_state_root(),
            RpcError::unknown_iterator(),
            RpcError::unknown_session(),
            RpcError::unknown_height(),
            RpcError::insufficient_funds_wallet(),
            RpcError::wallet_fee_limit(),
            RpcError::no_opened_wallet(),
            RpcError::wallet_not_found(),
            RpcError::wallet_not_supported(),
            RpcError::unknown_account(),
            RpcError::verification_failed(),
            RpcError::already_exists(),
            RpcError::mempool_cap_reached(),
            RpcError::already_in_pool(),
            RpcError::insufficient_network_fee(),
            RpcError::policy_failed(),
            RpcError::invalid_script(),
            RpcError::invalid_attribute(),
            RpcError::invalid_signature(),
            RpcError::invalid_size(),
            RpcError::expired_transaction(),
            RpcError::insufficient_funds(),
            RpcError::invalid_contract_verification(),
            RpcError::access_denied(),
            RpcError::sessions_disabled(),
            RpcError::oracle_disabled(),
            RpcError::oracle_request_finished(),
            RpcError::oracle_request_not_found(),
            RpcError::oracle_not_designated_node(),
            RpcError::unsupported_state(),
            RpcError::invalid_proof(),
            RpcError::execution_failed(),
        ];

        for error in errors.iter() {
            if error.code() == RpcError::wallet_fee_limit().code() {
                assert!(error.data().is_some());
            } else {
                assert!(error.data().is_none());
            }
        }
    }

    #[test]
    fn rpc_error_wallet_fee_limit_json_includes_data() {
        let error = RpcError::wallet_fee_limit();
        let json = error.to_json().to_string();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse json");
        let data = parsed.get("data").and_then(|v| v.as_str()).expect("data");
        assert_eq!(data, error.data().expect("data"));
        let message = parsed.get("message").and_then(|v| v.as_str()).expect("message");
        assert!(message.contains(error.message()));
        assert!(message.contains(data));
    }

    #[test]
    fn rpc_error_strings_are_unique() {
        let errors = vec![
            RpcError::invalid_request(),
            RpcError::method_not_found(),
            RpcError::invalid_params(),
            RpcError::internal_server_error(),
            RpcError::too_many_requests(),
            RpcError::bad_request(),
            RpcError::unknown_block(),
            RpcError::unknown_contract(),
            RpcError::unknown_transaction(),
            RpcError::unknown_storage_item(),
            RpcError::unknown_script_container(),
            RpcError::unknown_state_root(),
            RpcError::unknown_iterator(),
            RpcError::unknown_session(),
            RpcError::unknown_height(),
            RpcError::insufficient_funds_wallet(),
            RpcError::wallet_fee_limit(),
            RpcError::no_opened_wallet(),
            RpcError::wallet_not_found(),
            RpcError::wallet_not_supported(),
            RpcError::unknown_account(),
            RpcError::verification_failed(),
            RpcError::already_exists(),
            RpcError::mempool_cap_reached(),
            RpcError::already_in_pool(),
            RpcError::insufficient_network_fee(),
            RpcError::policy_failed(),
            RpcError::invalid_script(),
            RpcError::invalid_attribute(),
            RpcError::invalid_signature(),
            RpcError::invalid_size(),
            RpcError::expired_transaction(),
            RpcError::insufficient_funds(),
            RpcError::invalid_contract_verification(),
            RpcError::access_denied(),
            RpcError::sessions_disabled(),
            RpcError::oracle_disabled(),
            RpcError::oracle_request_finished(),
            RpcError::oracle_request_not_found(),
            RpcError::oracle_not_designated_node(),
            RpcError::unsupported_state(),
            RpcError::invalid_proof(),
            RpcError::execution_failed(),
        ];

        let mut seen = HashSet::new();
        for error in errors {
            let key = error.to_string();
            assert!(seen.insert(key));
        }
    }
}
