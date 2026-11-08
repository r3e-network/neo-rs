//! Stub implementations for signing types used by the oracle service.
//!
//! These are temporary stubs to satisfy compilation requirements.
//! Full implementations would connect to a real signing service.

use neo_primitives::UInt160;
use std::collections::{BTreeMap, BTreeSet};

/// Schema version for signing operations.
pub const SIGNING_SCHEMA_VERSION: u32 = 1;

/// Purpose of a signing operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignPurpose {
    /// Oracle response signing
    OracleResponse,
    /// Transaction signing
    Transaction,
    /// General message signing
    Message,
}

/// Payload to be signed.
#[derive(Debug, Clone)]
pub enum SignPayload {
    /// Canonical encoding with format string and bytes
    Canonical {
        /// Encoding format identifier
        encoding: String,
        /// Raw bytes to sign
        bytes: Vec<u8>,
    },
    /// Raw bytes without encoding metadata
    Raw(Vec<u8>),
}

/// Protocol context for signing operations.
#[derive(Debug, Clone)]
pub struct ProtocolContext {
    /// Schema version
    pub schema_version: u32,
    /// Protocol identifier
    pub protocol: String,
    /// Protocol version
    pub protocol_version: String,
    /// Additional protocol-specific fields
    pub fields: BTreeMap<String, String>,
}

/// Key reference for signing operations.
#[derive(Debug, Clone)]
pub struct KeyRef {
    /// Key identifier
    pub id: String,
    /// Key type
    pub key_type: String,
}

/// Frozen sign intent containing all parameters for a signing operation.
#[derive(Debug, Clone)]
pub struct FrozenSignIntent {
    /// Schema version
    pub schema_version: u32,
    /// Network magic number
    pub network: u32,
    /// Purpose of signing
    pub purpose: SignPurpose,
    /// Reference to the key to use
    pub key_ref: KeyRef,
    /// Payload to sign
    pub payload: SignPayload,
    /// Protocol context
    pub protocol_context: ProtocolContext,
    /// Policy version
    pub policy_version: String,
    /// Fee units
    pub fee_units: u64,
    /// Quota units
    pub quota_units: u64,
    /// Account selector
    pub account: Option<String>,
    /// Target accounts/contracts
    pub targets: BTreeSet<String>,
}

/// Response from a signing operation.
#[derive(Debug, Clone)]
pub struct SignResponse {
    /// The signature bytes
    pub signature: Vec<u8>,
    /// Operation ID
    pub operation_id: String,
}

/// Error type for signing operations.
#[derive(Debug, Clone)]
pub struct SigningError {
    /// Error message
    pub message: String,
}

impl std::fmt::Display for SigningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Signing error: {}", self.message)
    }
}

impl std::error::Error for SigningError {}

/// Client for interacting with a trusted signing service.
#[derive(Clone)]
pub struct TrustedSignerClient {
    /// Network magic number
    network: u32,
    /// Policy version
    policy_version: u32,
}

impl TrustedSignerClient {
    /// Creates a new trusted signer client.
    pub fn new(network: u32, policy_version: u32) -> Self {
        Self {
            network,
            policy_version,
        }
    }

    /// Gets the policy version.
    pub fn policy_version(&self) -> u32 {
        self.policy_version
    }

    /// Resolves a key reference for an account.
    pub fn resolve_key(
        &self,
        _account_selector: &str,
        _purpose: &SignPurpose,
    ) -> Result<KeyRef, SigningError> {
        // Stub implementation
        Ok(KeyRef {
            id: "stub-key".to_string(),
            key_type: "secp256r1".to_string(),
        })
    }

    /// Resumes or starts a signing operation.
    pub async fn resume_or_start(
        &self,
        operation_id: &str,
        _intent: FrozenSignIntent,
    ) -> Result<SignResponse, SigningError> {
        // Stub implementation - returns empty signature
        Ok(SignResponse {
            signature: vec![],
            operation_id: operation_id.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trusted_signer_client_creation() {
        let client = TrustedSignerClient::new(860833102, 1);
        assert_eq!(client.policy_version(), 1);
    }

    #[test]
    fn test_resolve_key() {
        let client = TrustedSignerClient::new(860833102, 1);
        let result = client.resolve_key("test-account", &SignPurpose::OracleResponse);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_resume_or_start() {
        let client = TrustedSignerClient::new(860833102, 1);
        let intent = FrozenSignIntent {
            schema_version: SIGNING_SCHEMA_VERSION,
            network: 860833102,
            purpose: SignPurpose::OracleResponse,
            key_ref: KeyRef {
                id: "test".to_string(),
                key_type: "secp256r1".to_string(),
            },
            payload: SignPayload::Raw(vec![1, 2, 3]),
            protocol_context: ProtocolContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                protocol: "neo3".to_string(),
                protocol_version: "1".to_string(),
                fields: BTreeMap::new(),
            },
            policy_version: "1".to_string(),
            fee_units: 0,
            quota_units: 1,
            account: None,
            targets: BTreeSet::new(),
        };

        let result = client.resume_or_start("op-123", intent).await;
        assert!(result.is_ok());
    }
}
