//! Server configuration for the reference remote signer.

use neo_wallets::signing::{KeyRef, SignPurpose};
use std::net::SocketAddr;
use std::path::PathBuf;

/// Mirrors the node client's `MAX_ATTESTATION_CLOCK_SKEW_MS` bound: the server
/// refuses challenges whose client timestamp is further than this from the
/// server clock.
pub const MAX_ATTESTATION_CLOCK_SKEW_MS: i64 = 60_000;

/// Default hard lifetime of one attestation session.
pub const DEFAULT_SESSION_LIFETIME_MS: i64 = 300_000;

/// Filesystem locations of the mutual-TLS material.
///
/// * server certificate/key: the HTTPS identity presented to node clients,
/// * CA certificate: the trust anchor client certificates must chain to.
/// Clients without a certificate issued by this CA are rejected during the
/// TLS handshake.
#[derive(Debug, Clone)]
pub struct TlsPaths {
    /// PEM file with the server certificate (leaf, optionally followed by the
    /// chain).
    pub server_certificate_path: PathBuf,
    /// PEM file with the server private key (PKCS#8/SEC1/RSA).
    pub server_private_key_path: PathBuf,
    /// PEM file with the CA certificate used to authenticate node clients.
    pub ca_certificate_path: PathBuf,
}

/// Fault-injection switches for integration tests.
///
/// These exist so tests can drive the node client's rejection and recovery
/// paths against a real mTLS server. They mirror the CLI flags of the
/// reference binary and MUST NOT be enabled in production deployments.
#[derive(Debug, Clone, Default)]
pub struct FaultPolicy {
    /// Offset (milliseconds) added to the `issued_at_unix_ms` of generated
    /// attestation evidence. Non-zero values simulate a signer whose clock
    /// is skewed beyond the client's `max_clock_skew_ms` bound, so the
    /// client must refuse the evidence.
    pub attest_clock_skew_ms: i64,
    /// Delay (milliseconds) before the attest endpoint answers. Combined
    /// with a small client `max_challenge_age_ms`, this makes the response
    /// arrive after the challenge freshness window and the client must
    /// refuse the evidence.
    pub attest_delay_ms: u64,
    /// When true, the first `sign` dispatch for a request records the
    /// operation as in-flight, does NOT sign, and answers HTTP 503 so the
    /// client classifies the result as `ResultUnknown`. The signing is then
    /// completed by the first `query` (which reports `Pending`), and the
    /// durable result is served by subsequent queries. This exercises the
    /// full `ResultUnknown` recovery path without ever re-signing.
    pub hold_dispatch: bool,
}

/// Complete configuration of the reference signer service.
#[derive(Debug, Clone)]
pub struct SignerServerConfig {
    /// Address the HTTPS listener binds to.
    pub bind_addr: SocketAddr,
    /// Mutual-TLS material locations.
    pub tls: TlsPaths,
    /// neo-tee enclave sealed-data directory (monotonic counter + rollback
    /// floor live here).
    pub sealed_data_dir: PathBuf,
    /// File the sealed signing key is persisted to.
    pub sealed_key_path: PathBuf,
    /// Service identity the server attests as; must equal the value mapped in
    /// the node's `identity_mapping` configuration.
    pub service_identity: String,
    /// Signing policy version; must equal the node's local policy version.
    pub policy_version: String,
    /// The single key reference this signer is authorized to serve.
    pub key_ref: KeyRef,
    /// Purposes this signer authorizes for [`Self::key_ref`]. Requests with
    /// any other purpose receive an authenticated permanent rejection.
    pub allowed_purposes: Vec<SignPurpose>,
    /// Hard session lifetime measured from evidence creation. The client's
    /// `max_session_age_ms` policy must dominate this value.
    pub session_lifetime_ms: i64,
    /// Server-side freshness bound applied to incoming attestation challenges
    /// (mirrors the node's `MAX_ATTESTATION_CLOCK_SKEW_MS`).
    pub max_challenge_age_ms: i64,
    /// Test-only fault-injection switches (see [`FaultPolicy`]).
    pub faults: FaultPolicy,
}

impl SignerServerConfig {
    /// Returns true if the purpose is authorized for [`Self::key_ref`].
    #[must_use]
    pub fn is_purpose_authorized(&self, purpose: &SignPurpose) -> bool {
        self.allowed_purposes.contains(purpose)
    }

    /// Returns the hard expiry (unix ms) of a session created at `issued_at`.
    #[must_use]
    pub fn session_expires_at_unix_ms(&self, issued_at_unix_ms: i64) -> i64 {
        issued_at_unix_ms.saturating_add(self.session_lifetime_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SignerServerConfig {
        SignerServerConfig {
            bind_addr: "127.0.0.1:0".parse().expect("bind address"),
            tls: TlsPaths {
                server_certificate_path: PathBuf::from("server.pem"),
                server_private_key_path: PathBuf::from("server.key"),
                ca_certificate_path: PathBuf::from("ca.pem"),
            },
            sealed_data_dir: PathBuf::from("sealed-data"),
            sealed_key_path: PathBuf::from("sealed-key.json"),
            service_identity: "signer.prod".to_string(),
            policy_version: "policy-v1".to_string(),
            key_ref: KeyRef::new("remote", "validator-1", 2),
            allowed_purposes: vec![SignPurpose::Consensus],
            session_lifetime_ms: DEFAULT_SESSION_LIFETIME_MS,
            max_challenge_age_ms: MAX_ATTESTATION_CLOCK_SKEW_MS,
            faults: FaultPolicy::default(),
        }
    }

    #[test]
    fn purpose_authorization_matches_the_whitelist_exactly() {
        let config = config();
        assert!(config.is_purpose_authorized(&SignPurpose::Consensus));
        assert!(!config.is_purpose_authorized(&SignPurpose::Transaction));
        assert!(!config.is_purpose_authorized(&SignPurpose::Custom("consensus".to_string())));
    }

    #[test]
    fn session_expiry_extends_by_the_configured_lifetime() {
        let config = config();
        assert_eq!(
            config.session_expires_at_unix_ms(1_000),
            1_000 + DEFAULT_SESSION_LIFETIME_MS
        );
    }
}
