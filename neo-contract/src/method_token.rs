//! MethodToken - matches C# Neo.SmartContract.MethodToken exactly.

use neo_primitives::UInt160;
use neo_vm::call_flags::CallFlags;
use serde::{Deserialize, Serialize};

/// Represents the methods that a contract will call statically (matches C# MethodToken).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MethodToken {
    /// The hash of the contract to be called.
    pub hash: UInt160,
    /// The name of the method to be called.
    pub method: String,
    /// The number of parameters of the method to be called.
    pub parameters_count: u16,
    /// Indicates whether the method to be called has a return value.
    pub has_return_value: bool,
    /// The CallFlags to be used to call the contract.
    pub call_flags: CallFlags,
}

/// Maximum length for method names.
pub const MAX_METHOD_NAME_LENGTH: usize = 32;

impl Default for MethodToken {
    fn default() -> Self {
        Self {
            hash: UInt160::zero(),
            method: String::new(),
            parameters_count: 0,
            has_return_value: false,
            call_flags: CallFlags::NONE,
        }
    }
}

impl MethodToken {
    /// Creates a new MethodToken with validation.
    pub fn new(
        hash: UInt160,
        method: String,
        parameters_count: u16,
        has_return_value: bool,
        call_flags: CallFlags,
    ) -> Result<Self, MethodTokenError> {
        // Validate method name doesn't start with underscore
        if method.starts_with('_') {
            return Err(MethodTokenError::InvalidMethodName(
                "Method name cannot start with underscore".to_string(),
            ));
        }

        // Validate method name length (max 32 chars in C#)
        if method.len() > MAX_METHOD_NAME_LENGTH {
            return Err(MethodTokenError::MethodNameTooLong(method.len()));
        }

        Ok(Self {
            hash,
            method,
            parameters_count,
            has_return_value,
            call_flags,
        })
    }

    /// Creates a new MethodToken without validation.
    pub fn new_unchecked(
        hash: UInt160,
        method: String,
        parameters_count: u16,
        has_return_value: bool,
        call_flags: CallFlags,
    ) -> Self {
        Self {
            hash,
            method,
            parameters_count,
            has_return_value,
            call_flags,
        }
    }

    /// Gets the size in bytes when serialized.
    pub fn size(&self) -> usize {
        20 + // UInt160 (hash)
        1 + self.method.len() + // VarString (method)
        2 + // u16 (parameters_count)
        1 + // bool (has_return_value)
        1 // CallFlags (1 byte)
    }

    /// Validates the method token.
    pub fn validate(&self) -> Result<(), MethodTokenError> {
        if self.method.starts_with('_') {
            return Err(MethodTokenError::InvalidMethodName(
                "Method name cannot start with underscore".to_string(),
            ));
        }
        if self.method.len() > MAX_METHOD_NAME_LENGTH {
            return Err(MethodTokenError::MethodNameTooLong(self.method.len()));
        }
        Ok(())
    }
}

/// Errors that can occur when working with MethodToken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MethodTokenError {
    /// Method name is invalid.
    InvalidMethodName(String),
    /// Method name exceeds maximum length.
    MethodNameTooLong(usize),
    /// Invalid call flags.
    InvalidCallFlags,
    /// Deserialization error.
    DeserializationError(String),
}

impl std::fmt::Display for MethodTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMethodName(msg) => write!(f, "Invalid method name: {msg}"),
            Self::MethodNameTooLong(len) => {
                write!(f, "Method name too long: {len} > {MAX_METHOD_NAME_LENGTH}")
            }
            Self::InvalidCallFlags => write!(f, "Invalid call flags"),
            Self::DeserializationError(msg) => write!(f, "Deserialization error: {msg}"),
        }
    }
}

impl std::error::Error for MethodTokenError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_token_default() {
        let token = MethodToken::default();
        assert_eq!(token.hash, UInt160::zero());
        assert_eq!(token.method, "");
        assert_eq!(token.parameters_count, 0);
        assert!(!token.has_return_value);
        assert_eq!(token.call_flags, CallFlags::NONE);
    }

    #[test]
    fn test_method_token_new_valid() {
        let hash = UInt160::zero();
        let token = MethodToken::new(hash, "transfer".to_string(), 3, true, CallFlags::ALL);
        assert!(token.is_ok());
        let token = token.unwrap();
        assert_eq!(token.method, "transfer");
        assert_eq!(token.parameters_count, 3);
        assert!(token.has_return_value);
    }

    #[test]
    fn test_method_token_new_underscore_prefix() {
        let hash = UInt160::zero();
        let token = MethodToken::new(hash, "_private".to_string(), 0, false, CallFlags::NONE);
        assert!(token.is_err());
        assert!(matches!(
            token.unwrap_err(),
            MethodTokenError::InvalidMethodName(_)
        ));
    }

    #[test]
    fn test_method_token_new_too_long() {
        let hash = UInt160::zero();
        let long_name = "a".repeat(33);
        let token = MethodToken::new(hash, long_name, 0, false, CallFlags::NONE);
        assert!(token.is_err());
        assert!(matches!(
            token.unwrap_err(),
            MethodTokenError::MethodNameTooLong(33)
        ));
    }

    #[test]
    fn test_method_token_size() {
        let token = MethodToken::new_unchecked(
            UInt160::zero(),
            "test".to_string(),
            0,
            false,
            CallFlags::NONE,
        );
        // 20 (hash) + 1 (len) + 4 (method) + 2 (params) + 1 (return) + 1 (flags) = 29
        assert_eq!(token.size(), 29);
    }

    #[test]
    fn test_method_token_validate() {
        let valid = MethodToken::new_unchecked(
            UInt160::zero(),
            "valid".to_string(),
            0,
            false,
            CallFlags::NONE,
        );
        assert!(valid.validate().is_ok());

        let invalid = MethodToken::new_unchecked(
            UInt160::zero(),
            "_invalid".to_string(),
            0,
            false,
            CallFlags::NONE,
        );
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_method_token_error_display() {
        let err = MethodTokenError::InvalidMethodName("test".to_string());
        assert!(err.to_string().contains("Invalid method name"));

        let err = MethodTokenError::MethodNameTooLong(50);
        assert!(err.to_string().contains("50"));
    }
}
