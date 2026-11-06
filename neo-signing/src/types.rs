//! Versioned data structures for policy-bound signing.

use super::error::{SignError, SignErrorCode, SignResult};
use neo_crypto::Crypto;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Current schema version for public signing structures.
pub const SIGNING_SCHEMA_VERSION: u16 = 1;

/// Neo N3 P-256 signature over the SHA-256 digest of canonical input bytes.
pub const ALGORITHM_ECDSA_P256_SHA256: &str = "ecdsa-p256-sha256";

/// Stable reference to a versioned provider key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct KeyRef {
    /// Structure schema version.
    pub schema_version: u16,
    /// Provider-specific namespace.
    pub provider: String,
    /// Provider-specific key identifier.
    pub key_id: String,
    /// Immutable key version requested for signing.
    pub version: u64,
}

impl KeyRef {
    /// Creates a versioned key reference.
    pub fn new(provider: impl Into<String>, key_id: impl Into<String>, version: u64) -> Self {
        Self {
            schema_version: SIGNING_SCHEMA_VERSION,
            provider: provider.into(),
            key_id: key_id.into(),
            version,
        }
    }

    /// Returns a stable selector used by policy and quota storage.
    #[must_use]
    pub fn selector(&self) -> String {
        format!("{}:{}:{}", self.provider, self.key_id, self.version)
    }
}

/// Security boundary protecting a signing key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityLevel {
    /// Key material is protected only by the host process.
    Software,
    /// Key material remains inside a local hardware security module.
    Hsm,
    /// Key material is sealed by the host TEE adapter but is not protected from
    /// a malicious host administrator.
    HostSealed,
    /// Signing executes in a locally attested hardware TEE.
    HardwareTee,
    /// Signing executes in a remotely attested hardware TEE session.
    RemoteHardwareTee,
    /// Key material is held by a disconnected or manually mediated provider.
    Offline,
}

/// Explicit protocol purpose for a signature.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignPurpose {
    /// Neo transaction authorization.
    Transaction,
    /// Consensus message authorization.
    Consensus,
    /// Oracle response authorization.
    OracleResponse,
    /// State-root authorization.
    StateRoot,
    /// Cross-chain protocol authorization.
    CrossChain,
    /// NeoFS bearer-token authorization using the protocol SHA-512 profile.
    NeoFsBearer,
    /// NeoFS WalletConnect authorization using the salted message profile.
    NeoFsWalletConnect,
    /// Deployment-specific, policy-named purpose.
    Custom(String),
}

impl SignPurpose {
    /// Returns the one exact authenticated scope required for this purpose.
    #[must_use]
    pub fn required_scope(&self) -> String {
        match self {
            Self::Transaction => "sign:transaction".to_string(),
            Self::Consensus => "sign:consensus".to_string(),
            Self::OracleResponse => "sign:oracle-response".to_string(),
            Self::StateRoot => "sign:state-root".to_string(),
            Self::CrossChain => "sign:cross-chain".to_string(),
            Self::NeoFsBearer => "sign:neofs-bearer".to_string(),
            Self::NeoFsWalletConnect => "sign:neofs-wallet-connect".to_string(),
            Self::Custom(name) => format!("sign:custom:{name}"),
        }
    }
}

/// One configured account/purpose mapping to an immutable provider key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SigningKeyBinding {
    /// Canonical Neo account or other policy account selector.
    pub account: String,
    /// Purpose for which this mapping is valid.
    pub purpose: SignPurpose,
    /// Exact configured provider key and immutable version.
    pub key_ref: KeyRef,
    /// Expected provider public key used to bind the account mapping.
    pub public_key: Vec<u8>,
}

/// Unified resolver for account/purpose to configured versioned provider keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SigningKeyResolver {
    bindings: BTreeMap<(String, SignPurpose), SigningKeyBinding>,
}

impl SigningKeyResolver {
    /// Builds a resolver and rejects ambiguous or incomplete bindings.
    pub fn new(bindings: impl IntoIterator<Item = SigningKeyBinding>) -> SignResult<Self> {
        let mut resolved = BTreeMap::new();
        for binding in bindings {
            if binding.account.trim().is_empty()
                || binding.key_ref.provider.trim().is_empty()
                || binding.key_ref.key_id.trim().is_empty()
                || binding.key_ref.version == 0
                || binding.public_key.len() != 33
            {
                return Err(SignError::new(
                    SignErrorCode::InvalidRequest,
                    "signing key binding requires an account, versioned key, and compressed P-256 public key",
                ));
            }
            let selector = (binding.account.clone(), binding.purpose.clone());
            if resolved.insert(selector, binding).is_some() {
                return Err(SignError::new(
                    SignErrorCode::InvalidRequest,
                    "duplicate signing account and purpose binding",
                ));
            }
        }
        Ok(Self { bindings: resolved })
    }

    /// Resolves an account and purpose to the exact configured key reference.
    pub fn resolve(&self, account: &str, purpose: &SignPurpose) -> SignResult<KeyRef> {
        self.bindings
            .get(&(account.to_string(), purpose.clone()))
            .map(|binding| binding.key_ref.clone())
            .ok_or_else(|| {
                SignError::new(
                    SignErrorCode::KeyNotFound,
                    "no configured signing key exists for the account and purpose",
                )
            })
    }

    /// Verifies that provider metadata remains bound to the configured account mapping.
    pub fn verify(&self, request: &SignRequest, key_info: &KeyInfo) -> SignResult<()> {
        let account = request.account.as_deref().ok_or_else(|| {
            SignError::new(
                SignErrorCode::PolicyDenied,
                "configured signing key resolution requires an account",
            )
            .for_request(request.request_id.clone())
        })?;
        let binding = self
            .bindings
            .get(&(account.to_string(), request.purpose.clone()))
            .ok_or_else(|| {
                SignError::new(
                    SignErrorCode::PolicyDenied,
                    "account and purpose have no configured signing key binding",
                )
                .for_request(request.request_id.clone())
            })?;
        if binding.key_ref != request.key_ref || binding.public_key != key_info.public_key {
            return Err(SignError::new(
                SignErrorCode::VerificationFailed,
                "provider key metadata does not match the configured account, purpose, key id, and version binding",
            )
            .for_request(request.request_id.clone()));
        }
        Ok(())
    }

    /// Returns all immutable bindings for policy construction and diagnostics.
    pub fn bindings(&self) -> impl Iterator<Item = &SigningKeyBinding> {
        self.bindings.values()
    }
}

/// Provenance governing how unverified post-dispatch signature material is classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderOutcomeProvenance {
    /// A local provider deterministically returned the observed material.
    DeterministicLocal,
    /// A remote dispatch may have completed differently from the observed response.
    RemoteUncertain,
}

/// Exact operations a provider is prepared to perform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    /// Provider name used by [`KeyRef::provider`].
    pub provider: String,
    /// Purposes accepted by this provider.
    pub purposes: BTreeSet<SignPurpose>,
    /// Canonical encoding identifiers accepted by this provider.
    pub canonical_encodings: BTreeSet<String>,
    /// Signature algorithms this provider can emit.
    pub algorithms: BTreeSet<String>,
    /// Accurately reported protection boundary.
    pub security_level: SecurityLevel,
    /// Explicit origin of post-dispatch outcome uncertainty.
    pub outcome_provenance: ProviderOutcomeProvenance,
}

impl ProviderCapabilities {
    /// Returns whether unverified returned signature material must remain recoverable.
    #[must_use]
    pub const fn has_remote_outcome_uncertainty(&self) -> bool {
        matches!(
            self.outcome_provenance,
            ProviderOutcomeProvenance::RemoteUncertain
        )
    }
}

impl ProviderCapabilities {
    /// Returns true only when every request dimension is explicitly supported.
    #[must_use]
    pub fn supports(&self, purpose: &SignPurpose, payload: &SignPayload, algorithm: &str) -> bool {
        if !self.purposes.contains(purpose) || !self.algorithms.contains(algorithm) {
            return false;
        }
        match payload {
            SignPayload::Canonical { encoding, .. } => self.canonical_encodings.contains(encoding),
            SignPayload::Digest { algorithm, .. } => self
                .canonical_encodings
                .contains(algorithm.capability_name()),
        }
    }
}

/// Metadata returned by a signing provider for a versioned key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyInfo {
    /// Structure schema version.
    pub schema_version: u16,
    /// Exact key reference described by this record.
    pub key_ref: KeyRef,
    /// Signature algorithm emitted by the provider.
    pub algorithm: String,
    /// Public key or verification material.
    pub public_key: Vec<u8>,
    /// Security boundary protecting the private key.
    pub security_level: SecurityLevel,
    /// Whether the provider permits this key version to sign.
    pub enabled: bool,
    /// Provider metadata safe to expose to policy code.
    pub attributes: BTreeMap<String, String>,
}

/// Supported digest description for pre-hashed payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DigestAlgorithm {
    /// SHA-256.
    Sha256,
}

impl DigestAlgorithm {
    /// Stable capability selector for this digest profile.
    #[must_use]
    pub const fn capability_name(self) -> &'static str {
        match self {
            Self::Sha256 => "digest:sha256",
        }
    }
}

/// Payload material accepted by the policy service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SignPayload {
    /// Canonical bytes produced by the named protocol encoding.
    Canonical {
        /// Non-empty canonical encoding identifier.
        encoding: String,
        /// Canonical encoded payload bytes.
        bytes: Vec<u8>,
    },
    /// A protocol-produced digest, still bound to all request context.
    Digest {
        /// Digest algorithm.
        algorithm: DigestAlgorithm,
        /// Digest bytes.
        digest: [u8; 32],
    },
}

impl SignPayload {
    /// Returns the SHA-256 digest represented by this payload.
    #[must_use]
    pub fn signing_digest(&self) -> [u8; 32] {
        match self {
            Self::Canonical { bytes, .. } => Crypto::sha256(bytes),
            Self::Digest { digest, .. } => *digest,
        }
    }

    /// Returns canonical bytes when the provider must perform protocol hashing.
    #[must_use]
    pub fn canonical_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Canonical { bytes, .. } => Some(bytes),
            Self::Digest { .. } => None,
        }
    }

    pub(crate) fn validate(&self) -> SignResult<()> {
        match self {
            Self::Canonical { encoding, bytes }
                if encoding.trim().is_empty() || bytes.is_empty() =>
            {
                Err(SignError::new(
                    SignErrorCode::InvalidRequest,
                    "canonical payload requires a named encoding and non-empty bytes",
                ))
            }
            _ => Ok(()),
        }
    }
}

/// Protocol-specific fields bound into the authorization decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolContext {
    /// Structure schema version.
    pub schema_version: u16,
    /// Protocol name, such as `neo-n3-transaction`.
    pub protocol: String,
    /// Protocol encoding or domain-separation version.
    pub protocol_version: String,
    /// Canonically ordered protocol fields.
    pub fields: BTreeMap<String, String>,
}

/// Idempotency and replay-prevention material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdempotencyInfo {
    /// Structure schema version.
    pub schema_version: u16,
    /// Caller-generated stable idempotency key.
    pub key: String,
    /// Single-use replay nonce scoped by tenant.
    pub replay_nonce: String,
}

/// Authenticated caller information supplied by a trusted integration layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticatedContext {
    /// Structure schema version.
    pub schema_version: u16,
    /// Authenticated principal identifier.
    pub principal: String,
    /// Authenticated tenant identifier.
    pub tenant: String,
    /// Authentication mechanism or assurance profile.
    pub authentication_method: String,
    /// Granted authorization scopes.
    pub scopes: BTreeSet<String>,
    /// Additional non-secret, authenticated claims.
    pub claims: BTreeMap<String, String>,
}

/// Immutable business intent used to resume an existing operation or create it once.
///
/// The operation identifier, authenticated caller, and first absolute deadline are
/// supplied by the trusted client and signing service. Every business-controlled
/// authorization dimension is frozen here and must match a durable operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenSignIntent {
    /// Structure schema version.
    pub schema_version: u16,
    /// Network magic or other stable network identifier.
    pub network: u32,
    /// Explicit signature purpose.
    pub purpose: SignPurpose,
    /// Exact key and key version requested.
    pub key_ref: KeyRef,
    /// Canonical payload or protocol-produced digest.
    pub payload: SignPayload,
    /// Protocol-specific domain context.
    pub protocol_context: ProtocolContext,
    /// Exact policy version expected by the caller.
    pub policy_version: String,
    /// Integer fee units associated with the operation.
    pub fee_units: u64,
    /// Integer quota units to reserve atomically.
    pub quota_units: u64,
    /// Optional account selector for policy evaluation.
    pub account: Option<String>,
    /// Target selectors for allow/deny-list evaluation.
    pub targets: BTreeSet<String>,
}

impl FrozenSignIntent {
    /// Creates the sole complete request for a previously unseen operation.
    pub fn into_request(
        self,
        operation_id: impl Into<String>,
        deadline_unix_ms: i64,
    ) -> SignResult<SignRequest> {
        let operation_id = operation_id.into();
        if operation_id.trim().is_empty() {
            return Err(SignError::new(
                SignErrorCode::InvalidRequest,
                "signing operation id must not be empty",
            ));
        }
        Ok(SignRequest {
            schema_version: self.schema_version,
            request_id: operation_id.clone(),
            network: self.network,
            purpose: self.purpose,
            key_ref: self.key_ref,
            payload: self.payload,
            protocol_context: self.protocol_context,
            policy_version: self.policy_version,
            deadline_unix_ms,
            idempotency: IdempotencyInfo {
                schema_version: SIGNING_SCHEMA_VERSION,
                key: operation_id.clone(),
                replay_nonce: operation_id,
            },
            fee_units: self.fee_units,
            quota_units: self.quota_units,
            account: self.account,
            targets: self.targets,
        })
    }

    /// Verifies an existing durable request without refreshing its first deadline.
    pub fn verify_existing(&self, operation_id: &str, request: &SignRequest) -> SignResult<()> {
        let matches = request.request_id == operation_id
            && request.idempotency.schema_version == SIGNING_SCHEMA_VERSION
            && request.idempotency.key == operation_id
            && request.idempotency.replay_nonce == operation_id
            && request.schema_version == self.schema_version
            && request.network == self.network
            && request.purpose == self.purpose
            && request.key_ref == self.key_ref
            && request.payload == self.payload
            && request.protocol_context == self.protocol_context
            && request.policy_version == self.policy_version
            && request.fee_units == self.fee_units
            && request.quota_units == self.quota_units
            && request.account == self.account
            && request.targets == self.targets;
        if !matches {
            return Err(SignError::new(
                SignErrorCode::IdempotencyConflict,
                "operation intent does not match the complete first persisted authorization binding",
            )
            .for_request(operation_id.to_string()));
        }
        Ok(())
    }
}

/// Fully bound request accepted by a [`SignerService`](super::SignerService).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignRequest {
    /// Structure schema version.
    pub schema_version: u16,
    /// Unique caller request identifier.
    pub request_id: String,
    /// Network magic or other stable network identifier.
    pub network: u32,
    /// Explicit signature purpose.
    pub purpose: SignPurpose,
    /// Exact key and key version requested.
    pub key_ref: KeyRef,
    /// Canonical payload or protocol-produced digest.
    pub payload: SignPayload,
    /// Protocol-specific domain context.
    pub protocol_context: ProtocolContext,
    /// Exact policy version expected by the caller.
    pub policy_version: String,
    /// Absolute Unix timestamp deadline in milliseconds.
    pub deadline_unix_ms: i64,
    /// Idempotency and replay-prevention material.
    pub idempotency: IdempotencyInfo,
    /// Integer fee units associated with the operation.
    pub fee_units: u64,
    /// Integer quota units to reserve atomically.
    pub quota_units: u64,
    /// Optional account selector for policy evaluation.
    pub account: Option<String>,
    /// Target selectors for allow/deny-list evaluation.
    pub targets: BTreeSet<String>,
}

impl SignRequest {
    pub(crate) fn validate(&self, context: &AuthenticatedContext) -> SignResult<()> {
        let invalid = self.schema_version != SIGNING_SCHEMA_VERSION
            || self.key_ref.schema_version != SIGNING_SCHEMA_VERSION
            || self.protocol_context.schema_version != SIGNING_SCHEMA_VERSION
            || self.idempotency.schema_version != SIGNING_SCHEMA_VERSION
            || context.schema_version != SIGNING_SCHEMA_VERSION
            || self.request_id.trim().is_empty()
            || self.key_ref.provider.trim().is_empty()
            || self.key_ref.key_id.trim().is_empty()
            || self.key_ref.version == 0
            || self.protocol_context.protocol.trim().is_empty()
            || self.protocol_context.protocol_version.trim().is_empty()
            || self.policy_version.trim().is_empty()
            || self.idempotency.key.trim().is_empty()
            || self.idempotency.replay_nonce.trim().is_empty()
            || context.principal.trim().is_empty()
            || context.tenant.trim().is_empty()
            || context.authentication_method.trim().is_empty()
            || self.quota_units == 0;
        if invalid {
            return Err(SignError::new(
                SignErrorCode::InvalidRequest,
                "signing request contains missing fields or unsupported schema versions",
            )
            .for_request(self.request_id.clone()));
        }
        self.payload
            .validate()
            .map_err(|error| error.for_request(self.request_id.clone()))
    }

    /// Produces a stable digest of every request and caller field relevant to authorization.
    ///
    /// The absolute deadline is part of the authorization binding. Callers must reuse
    /// the first deadline assigned to an operation rather than refreshing it on retry.
    pub fn binding_digest(&self, context: &AuthenticatedContext) -> SignResult<[u8; 32]> {
        let bytes = serde_json::to_vec(&(self, context)).map_err(|error| {
            SignError::new(
                SignErrorCode::InvalidRequest,
                format!("failed to canonicalize request binding: {error}"),
            )
            .for_request(self.request_id.clone())
        })?;
        Ok(Crypto::sha256(&bytes))
    }
}

/// Provider-produced signature material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSignature {
    /// Signature algorithm used by the provider.
    pub algorithm: String,
    /// Provider-produced signature bytes.
    pub signature: Vec<u8>,
}

/// Successful, versioned signing response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignResponse {
    /// Structure schema version.
    pub schema_version: u16,
    /// Original request identifier.
    pub request_id: String,
    /// Key and version used for signing.
    pub key_ref: KeyRef,
    /// Signature algorithm.
    pub algorithm: String,
    /// Signature bytes.
    pub signature: Vec<u8>,
    /// Digest supplied to the provider.
    pub signed_digest: [u8; 32],
    /// Stable digest binding the logical request and authenticated context.
    pub request_digest: [u8; 32],
    /// Policy version that authorized the operation.
    pub policy_version: String,
    /// Completion timestamp in Unix milliseconds.
    pub completed_at_unix_ms: i64,
    /// True when returned from persistent idempotency state.
    pub idempotent_replay: bool,
}
