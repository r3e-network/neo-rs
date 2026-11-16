//! Signer-service configuration sections.
//!
//! Plain serde data types bound to the unified signing contracts. The node
//! configuration facade re-exports them so historical `crate::config::*`
//! paths remain valid (Phase 3 S6a sink-down).

use super::{SecurityLevel, SignPurpose};
use serde::Deserialize;
use std::collections::BTreeMap;

/// Unified signing service configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SignerServiceSection {
    /// Enables policy-bound signer registration and production entry points.
    pub enabled: bool,
    /// Append-only journal path used for idempotency, quota, and audit state.
    pub state_path: String,
    /// Stable policy version bound to every request.
    pub policy_version: String,
    /// Maximum provider round-trip time for one request.
    pub request_timeout_ms: u64,
    /// Maximum fee units accepted by the node signing policy.
    pub max_fee_units: u64,
    /// Maximum signatures in one quota window.
    pub quota_limit_units: u64,
    /// Quota window duration in milliseconds.
    pub quota_window_ms: i64,
    /// Providers available to this node.
    pub providers: Vec<SignerProviderSection>,
    /// Purposes the default provider must explicitly support.
    pub required_purposes: Vec<SignPurpose>,
    /// Canonical encodings the default provider must explicitly support.
    pub required_canonical_encodings: Vec<String>,
    /// Signature algorithms the default provider must explicitly support.
    pub required_algorithms: Vec<String>,
    /// Exact boundary required of the default provider.
    pub required_security_level: Option<SecurityLevel>,
}

impl Default for SignerServiceSection {
    fn default() -> Self {
        Self {
            enabled: false,
            state_path: String::new(),
            policy_version: "node-signing-v1".to_string(),
            request_timeout_ms: 5_000,
            max_fee_units: 0,
            quota_limit_units: 0,
            quota_window_ms: 60_000,
            providers: Vec::new(),
            required_purposes: Vec::new(),
            required_canonical_encodings: Vec::new(),
            required_algorithms: Vec::new(),
            required_security_level: None,
        }
    }
}

/// Configured signer provider implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SignerProviderKind {
    /// Local NEP-6 software wallet.
    #[default]
    Nep6,
    /// Local HSM wallet adapter.
    Hsm,
    /// Existing host-sealed TEE wallet adapter.
    HostSealedTee,
    /// Locally attested hardware TEE.
    HardwareTee,
    /// Dedicated mTLS remote signer without a hardware-attestation claim.
    Remote,
    /// Remotely attested hardware TEE signer requiring a production verifier.
    RemoteTee,
}

/// One explicitly capability-scoped signer provider.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct SignerProviderSection {
    /// Stable provider identifier used by `KeyRef`.
    pub id: String,
    /// Provider implementation type.
    #[serde(rename = "type")]
    pub kind: SignerProviderKind,
    /// Whether this provider is available for registration.
    pub enabled: bool,
    /// Whether this is the sole default provider.
    pub default: bool,
    /// Accurately declared key protection boundary.
    pub security_level: Option<SecurityLevel>,
    /// Explicitly supported purposes.
    pub purposes: Vec<SignPurpose>,
    /// Explicitly supported canonical encodings.
    pub canonical_encodings: Vec<String>,
    /// Explicitly supported signature algorithms.
    pub algorithms: Vec<String>,
    /// Unified immutable account/purpose key mappings for this provider.
    pub keys: Vec<SignerKeySection>,
    /// Remote HTTPS endpoint used only by the dedicated signer client.
    pub endpoint: Option<String>,
    /// Dedicated mTLS and optional hardware-attestation configuration.
    pub remote: Option<RemoteSignerSection>,
    /// PKCS#11 HSM backend configuration for `hardware_tee` providers.
    pub pkcs11: Option<Pkcs11ProviderSection>,
}

/// Dedicated remote signer transport configuration.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct RemoteSignerSection {
    /// DNS name required in the server certificate and endpoint URL.
    pub server_name: String,
    /// PEM trust anchor file; platform roots are deliberately disabled.
    pub ca_certificate_path: String,
    /// PEM client certificate chain.
    pub client_certificate_path: String,
    /// PEM client private key.
    pub client_private_key_path: String,
    /// Application identity expected from every remote response.
    pub service_identity: String,
    /// Explicit service-identity to local-provider mapping.
    pub identity_mapping: BTreeMap<String, String>,
    /// Hardware evidence policy, required only for `remote_tee`.
    pub attestation: Option<RemoteAttestationPolicySection>,
}

/// PKCS#11 backend configuration for a `hardware_tee` signer provider.
///
/// The HSM user PIN is deliberately absent: it is only ever read from the
/// `NEO_HSM_PIN` environment variable so no secret lands in TOML files.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Pkcs11ProviderSection {
    /// Path to the PKCS#11 shared library providing `C_GetFunctionList`.
    pub library_path: String,
    /// Slot identifier holding the token with the signing key.
    pub slot_id: u64,
    /// Token label that must match the discovered token.
    pub token_label: String,
    /// `CKA_LABEL` of the EC P-256 private/public key pair.
    pub key_label: String,
}

/// One immutable configured signer key mapping shared by every provider kind.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct SignerKeySection {
    /// Provider-specific immutable key identifier.
    pub key_id: String,
    /// Positive immutable key version.
    pub version: u64,
    /// Expected compressed P-256 public key encoded as hex.
    pub public_key_hex: String,
    /// Canonical Neo account/script-hash selector bound to this key.
    pub account: String,
    /// Explicit purposes for which this account/key mapping is valid.
    pub purposes: Vec<SignPurpose>,
}

/// Strict hardware-attestation policy for a remote TEE.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct RemoteAttestationPolicySection {
    /// Explicit verifier implementation name; there is no permissive default.
    pub verifier: String,
    /// Exact accepted enclave/code measurement encoded as hex.
    pub measurement_hex: String,
    /// Optional exact accepted signer measurement encoded as hex.
    pub signer_measurement_hex: Option<String>,
    /// Exact accepted hardware product identifier.
    pub product_id: u16,
    /// Minimum accepted hardware security version.
    pub minimum_security_version: u16,
    /// Maximum challenge round-trip age.
    pub max_challenge_age_ms: u64,
    /// Maximum accepted positive or negative evidence clock skew.
    pub max_clock_skew_ms: u64,
    /// Maximum attested session lifetime.
    pub max_session_age_ms: u64,
}
