//! Production oracle signing implementation.
//!
//! Provides cryptographically secure ECDSA signing for Oracle responses using
//! secp256r1 (P-256/NIST) curve. Keypairs are generated on-demand and cached
//! in memory per account.
//!
//! Reference: Neo C# `OracleService` signing flow in Neo.Plugins.OracleService

use neo_crypto::Secp256r1Crypto;
use neo_primitives::UInt160;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

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
///
/// Provides cryptographically secure ECDSA signing for Oracle responses.
/// Keypairs are generated using `neo-crypto::Secp256r1Crypto` and cached
/// in memory per account.
#[derive(Clone)]
pub struct TrustedSignerClient {
    /// Network magic number
    network: u32,
    /// Policy version
    policy_version: u32,
    /// In-memory keypair cache (account selector -> private key)
    keypairs: Arc<RwLock<BTreeMap<String, [u8; 32]>>>,
}

impl TrustedSignerClient {
    /// Creates a new trusted signer client.
    pub fn new(network: u32, policy_version: u32) -> Self {
        Self {
            network,
            policy_version,
            keypairs: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    /// Gets the policy version.
    pub fn policy_version(&self) -> u32 {
        self.policy_version
    }

    /// Resolves a key reference for an account.
    ///
    /// Loads an existing keypair from cache or generates a new secp256r1 keypair
    /// and caches it for future use. This matches Neo's oracle signing behavior
    /// where each oracle node has a persistent keypair for signing responses.
    ///
    /// # Reference
    /// Neo C# `OracleService` - oracle nodes use their wallet keys for signing
    pub fn resolve_key(
        &self,
        account_selector: &str,
        _purpose: &SignPurpose,
    ) -> Result<KeyRef, SigningError> {
        // Check if we already have a keypair for this account
        {
            let cache = self.keypairs.read().map_err(|e| SigningError {
                message: format!("Failed to acquire read lock: {}", e),
            })?;
            if cache.contains_key(account_selector) {
                return Ok(KeyRef {
                    id: format!("oracle-key-{}", account_selector),
                    key_type: "secp256r1".to_string(),
                });
            }
        }

        // Generate new keypair and cache it
        let private_key = Secp256r1Crypto::generate_private_key();
        let mut cache = self.keypairs.write().map_err(|e| SigningError {
            message: format!("Failed to acquire write lock: {}", e),
        })?;
        cache.insert(account_selector.to_string(), private_key);

        Ok(KeyRef {
            id: format!("oracle-key-{}", account_selector),
            key_type: "secp256r1".to_string(),
        })
    }

    /// Resumes or starts a signing operation.
    ///
    /// Performs ECDSA signing using secp256r1 (P-256) curve. The signature is
    /// produced over the SHA-256 hash of the payload using RFC 6979 deterministic
    /// nonces and normalized to low-S form for canonical Neo N3 signatures.
    ///
    /// # Reference
    /// - Neo C# `Crypto.Sign()` - secp256r1 ECDSA with SHA-256
    /// - Neo.Cryptography.ECC.ECDsa - signature generation and verification
    pub async fn resume_or_start(
        &self,
        operation_id: &str,
        intent: FrozenSignIntent,
    ) -> Result<SignResponse, SigningError> {
        // Extract the account selector to look up the keypair
        let account_selector = intent.account.ok_or_else(|| SigningError {
            message: "No account selector in sign intent".to_string(),
        })?;

        // Get the private key from cache
        let private_key = {
            let cache = self.keypairs.read().map_err(|e| SigningError {
                message: format!("Failed to acquire read lock: {}", e),
            })?;
            cache.get(&account_selector).copied().ok_or_else(|| {
                SigningError {
                    message: format!("No keypair found for account {}", account_selector),
                }
            })?
        };

        // Extract the payload bytes
        let bytes = match intent.payload {
            SignPayload::Canonical { bytes, .. } => bytes,
            SignPayload::Raw(bytes) => bytes,
        };

        // Sign the payload using secp256r1
        // Secp256r1Crypto::sign() internally:
        // 1. Hashes the message with SHA-256
        // 2. Signs with ECDSA using RFC 6979 deterministic nonces
        // 3. Normalizes to low-S form for canonical signatures
        let signature = Secp256r1Crypto::sign(&bytes, &private_key).map_err(|e| {
            SigningError {
                message: format!("Signing failed: {}", e),
            }
        })?;

        Ok(SignResponse {
            signature: signature.to_vec(),
            operation_id: operation_id.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_crypto::{Crypto, Secp256r1Crypto};

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

        let key_ref = result.unwrap();
        assert_eq!(key_ref.key_type, "secp256r1");
        assert!(key_ref.id.starts_with("oracle-key-"));
    }

    #[test]
    fn test_resolve_key_caching() {
        let client = TrustedSignerClient::new(860833102, 1);

        // Resolve key for same account twice
        let result1 = client.resolve_key("test-account", &SignPurpose::OracleResponse);
        let result2 = client.resolve_key("test-account", &SignPurpose::OracleResponse);

        assert!(result1.is_ok());
        assert!(result2.is_ok());

        // Should return same key reference (cached keypair)
        let key_ref1 = result1.unwrap();
        let key_ref2 = result2.unwrap();
        assert_eq!(key_ref1.id, key_ref2.id);
    }

    #[tokio::test]
    async fn test_resume_or_start_produces_valid_signature() {
        let client = TrustedSignerClient::new(860833102, 1);

        // First resolve key to generate keypair
        let key_ref = client
            .resolve_key("test-account", &SignPurpose::OracleResponse)
            .expect("resolve key");

        // Create sign intent
        let message = b"test oracle response payload";
        let intent = FrozenSignIntent {
            schema_version: SIGNING_SCHEMA_VERSION,
            network: 860833102,
            purpose: SignPurpose::OracleResponse,
            key_ref,
            payload: SignPayload::Raw(message.to_vec()),
            protocol_context: ProtocolContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                protocol: "neo3".to_string(),
                protocol_version: "1".to_string(),
                fields: BTreeMap::new(),
            },
            policy_version: "1".to_string(),
            fee_units: 0,
            quota_units: 1,
            account: Some("test-account".to_string()),
            targets: BTreeSet::new(),
        };

        // Sign the message
        let result = client.resume_or_start("op-123", intent).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.operation_id, "op-123");
        assert_eq!(response.signature.len(), 64); // ECDSA signature is 64 bytes (r||s)

        // Verify the signature is valid
        // Get the private key to derive the public key for verification
        let private_key = {
            let cache = client.keypairs.read().unwrap();
            cache.get("test-account").copied().expect("cached key")
        };

        let public_key = Secp256r1Crypto::derive_public_key(&private_key).expect("public key");

        // Verify signature
        let is_valid = Secp256r1Crypto::verify(message, &response.signature.try_into().unwrap(), &public_key).expect("verify");
        assert!(is_valid, "Signature should be valid");
    }

    #[tokio::test]
    async fn test_deterministic_signatures() {
        let client = TrustedSignerClient::new(860833102, 1);

        // Resolve key
        let key_ref = client
            .resolve_key("test-account-2", &SignPurpose::OracleResponse)
            .expect("resolve key");

        let message = b"deterministic test message";
        let intent1 = FrozenSignIntent {
            schema_version: SIGNING_SCHEMA_VERSION,
            network: 860833102,
            purpose: SignPurpose::OracleResponse,
            key_ref: key_ref.clone(),
            payload: SignPayload::Raw(message.to_vec()),
            protocol_context: ProtocolContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                protocol: "neo3".to_string(),
                protocol_version: "1".to_string(),
                fields: BTreeMap::new(),
            },
            policy_version: "1".to_string(),
            fee_units: 0,
            quota_units: 1,
            account: Some("test-account-2".to_string()),
            targets: BTreeSet::new(),
        };

        let intent2 = intent1.clone();

        // Sign same message twice with same key
        let result1 = client.resume_or_start("op-1", intent1).await.expect("sign 1");
        let result2 = client.resume_or_start("op-2", intent2).await.expect("sign 2");

        // RFC 6979 deterministic signatures - same message + key = same signature
        assert_eq!(result1.signature, result2.signature);
    }

    #[tokio::test]
    async fn test_canonical_payload_encoding() {
        let client = TrustedSignerClient::new(860833102, 1);

        let key_ref = client
            .resolve_key("test-account-3", &SignPurpose::OracleResponse)
            .expect("resolve key");

        let message = b"canonical encoding test";
        let intent = FrozenSignIntent {
            schema_version: SIGNING_SCHEMA_VERSION,
            network: 860833102,
            purpose: SignPurpose::OracleResponse,
            key_ref,
            payload: SignPayload::Canonical {
                encoding: "neo-n3-sign-data-v1".to_string(),
                bytes: message.to_vec(),
            },
            protocol_context: ProtocolContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                protocol: "neo3".to_string(),
                protocol_version: "1".to_string(),
                fields: BTreeMap::new(),
            },
            policy_version: "1".to_string(),
            fee_units: 0,
            quota_units: 1,
            account: Some("test-account-3".to_string()),
            targets: BTreeSet::new(),
        };

        let result = client.resume_or_start("op-canonical", intent).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.signature.len(), 64);
    }
}
