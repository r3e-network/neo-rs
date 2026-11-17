//! Server configuration for the reference remote signer.

use anyhow::{Context, Result};
use neo_wallets::signing::{KeyRef, SignPurpose};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// Mirrors the node client's `MAX_ATTESTATION_CLOCK_SKEW_MS` bound: the server
/// refuses challenges whose client timestamp is further than this from the
/// server clock.
pub const MAX_ATTESTATION_CLOCK_SKEW_MS: i64 = 60_000;

/// Default hard lifetime of one attestation session.
pub const DEFAULT_SESSION_LIFETIME_MS: i64 = 300_000;

/// Environment variable pointing at the client-authorization policy file.
///
/// It takes precedence over the `client-authorization.json` file inside
/// [`SignerServerConfig::sealed_data_dir`].
pub const CLIENT_AUTHORIZATION_ENV: &str = "NEO_SIGNER_CLIENT_AUTHORIZATION";

/// File name of the client-authorization policy inside the sealed-data
/// directory.
pub const CLIENT_AUTHORIZATION_FILE: &str = "client-authorization.json";

/// Common name that matches every CA-authenticated client identity.
pub const ANY_IDENTITY: &str = "*";

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

    /// Path of the client-authorization policy file inside the sealed-data
    /// directory (used when [`CLIENT_AUTHORIZATION_ENV`] is not set).
    #[must_use]
    pub fn client_authorization_path(&self) -> PathBuf {
        self.sealed_data_dir.join(CLIENT_AUTHORIZATION_FILE)
    }

    /// Returns the hard expiry (unix ms) of a session created at `issued_at`.
    #[must_use]
    pub fn session_expires_at_unix_ms(&self, issued_at_unix_ms: i64) -> i64 {
        issued_at_unix_ms.saturating_add(self.session_lifetime_ms)
    }
}

/// One client-identity grant.
///
/// The identity is the subject common name of the client certificate the peer
/// presented during the TLS handshake. [`ANY_IDENTITY`] (`"*"`) matches every
/// CA-authenticated identity and is only used by the single-tenant default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientGrant {
    /// Exact certificate common name, or [`ANY_IDENTITY`].
    pub common_name: String,
    /// Key identifiers this identity may use, written as
    /// `"<provider>:<key_id>"`. [`ANY_IDENTITY`] (`"*"`) matches every key.
    pub allowed_key_ids: Vec<String>,
    /// Purposes this identity may request for those key IDs. An empty list
    /// means "every purpose", leaving the decision to the server-wide
    /// [`SignerServerConfig::allowed_purposes`] policy (which answers with an
    /// authenticated permanent rejection instead of a `403`).
    #[serde(default)]
    pub allowed_purposes: Vec<SignPurpose>,
}

impl ClientGrant {
    /// Builds a grant for one identity.
    #[must_use]
    pub fn new(
        common_name: impl Into<String>,
        allowed_key_ids: Vec<String>,
        allowed_purposes: Vec<SignPurpose>,
    ) -> Self {
        Self {
            common_name: common_name.into(),
            allowed_key_ids,
            allowed_purposes,
        }
    }

    /// True when this grant covers the client identity.
    #[must_use]
    fn covers_identity(&self, common_name: &str) -> bool {
        self.common_name == ANY_IDENTITY || self.common_name == common_name
    }

    /// True when the grant covers `key_ref` and `purpose`.
    #[must_use]
    fn allows(&self, key_ref: &KeyRef, purpose: Option<&SignPurpose>) -> bool {
        let key_id = key_selector(key_ref);
        if !self
            .allowed_key_ids
            .iter()
            .any(|allowed| allowed == ANY_IDENTITY || *allowed == key_id)
        {
            return false;
        }
        match purpose {
            // Metadata and recovery endpoints only need key scope; the
            // purpose of a dispatch is checked against the server policy.
            None => true,
            Some(_) if self.allowed_purposes.is_empty() => true,
            Some(purpose) => self
                .allowed_purposes
                .iter()
                .any(|allowed| allowed == purpose),
        }
    }
}

/// Returns the `"<provider>:<key_id>"` selector a grant authorizes.
#[must_use]
pub fn key_selector(key_ref: &KeyRef) -> String {
    format!("{}:{}", key_ref.provider, key_ref.key_id)
}

/// Identity-scoped authorization for the mTLS listener.
///
/// Authentication ("this client chains to the configured CA") is not
/// authorization ("this client may sign with this key for this purpose"):
/// every accepted certificate is scoped to the key IDs and purposes granted to
/// its common name. An identity without a matching grant is refused with HTTP
/// 403 before any signing operation is attempted.
///
/// The policy is loaded from, in order of precedence:
///
/// 1. the file named by [`CLIENT_AUTHORIZATION_ENV`],
/// 2. `client-authorization.json` inside the sealed-data directory,
/// 3. the single-tenant fallback ([`Self::single_tenant`]).
///
/// A configured policy is deny-by-default: identities that are not listed
/// cannot sign anything. The single-tenant fallback scopes every
/// CA-authenticated identity to exactly the one key this signer serves (with
/// no per-identity purpose narrowing, so the server-wide purpose whitelist
/// keeps answering with an authenticated permanent rejection); multi-tenant
/// deployments must install an explicit policy that lists the purposes each
/// identity may request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientAuthorizationPolicy {
    /// Explicit grants. Same-severity grants are additive per identity.
    #[serde(default)]
    pub grants: Vec<ClientGrant>,
}

impl ClientAuthorizationPolicy {
    /// Policy that authorizes every CA-authenticated client for exactly the
    /// configured key.
    ///
    /// Purposes are deliberately not narrowed here: a single-tenant signer
    /// already enforces [`SignerServerConfig::allowed_purposes`], and a
    /// purpose outside that whitelist must stay an authenticated permanent
    /// rejection (a signed `authorization_denied`) rather than an anonymous
    /// `403`. Per-identity purpose scoping is what an explicit policy adds.
    #[must_use]
    pub fn single_tenant(key_ref: &KeyRef) -> Self {
        Self {
            grants: vec![ClientGrant::new(
                ANY_IDENTITY,
                vec![key_selector(key_ref)],
                Vec::new(),
            )],
        }
    }

    /// Reads a policy from a JSON file.
    pub fn from_json_file(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path).with_context(|| {
            format!(
                "failed to read the client-authorization policy {}",
                path.display()
            )
        })?;
        let policy: Self = serde_json::from_str(&contents).with_context(|| {
            format!(
                "failed to parse the client-authorization policy {}",
                path.display()
            )
        })?;
        Ok(policy)
    }

    /// Loads the effective policy for `config`.
    ///
    /// A configured policy file that cannot be read or parsed fails the boot
    /// instead of silently widening access.
    pub fn load(config: &SignerServerConfig) -> Result<Self> {
        if let Some(path) = std::env::var_os(CLIENT_AUTHORIZATION_ENV) {
            let path = PathBuf::from(path);
            return Self::from_json_file(&path).with_context(|| {
                format!("{CLIENT_AUTHORIZATION_ENV} names an unusable policy file")
            });
        }
        let path = config.client_authorization_path();
        if path.exists() {
            return Self::from_json_file(&path);
        }
        Ok(Self::single_tenant(&config.key_ref))
    }

    /// True when the identity is authorized to use `key_ref` for `purpose`.
    #[must_use]
    pub fn is_authorized(
        &self,
        common_name: &str,
        key_ref: &KeyRef,
        purpose: Option<&SignPurpose>,
    ) -> bool {
        self.grants
            .iter()
            .any(|grant| grant.covers_identity(common_name) && grant.allows(key_ref, purpose))
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

    #[test]
    fn an_explicit_policy_is_deny_by_default() {
        let policy = ClientAuthorizationPolicy {
            grants: vec![ClientGrant::new(
                "neo-node-1",
                vec![key_selector(&KeyRef::new("remote", "validator-1", 2))],
                vec![SignPurpose::Consensus],
            )],
        };
        let key = KeyRef::new("remote", "validator-1", 2);
        assert!(policy.is_authorized("neo-node-1", &key, Some(&SignPurpose::Consensus)));
        // Unknown identity, foreign key and unlisted purpose are all denied.
        assert!(!policy.is_authorized("neo-node-2", &key, Some(&SignPurpose::Consensus)));
        assert!(!policy.is_authorized(
            "neo-node-1",
            &KeyRef::new("remote", "validator-2", 2),
            Some(&SignPurpose::Consensus)
        ));
        assert!(!policy.is_authorized("neo-node-1", &key, Some(&SignPurpose::Transaction)));
        assert!(!ClientAuthorizationPolicy::default().is_authorized("neo-node-1", &key, None));
    }

    #[test]
    fn the_single_tenant_default_grants_only_the_configured_key() {
        let config = config();
        let policy = ClientAuthorizationPolicy::single_tenant(&config.key_ref);
        assert!(policy.is_authorized(
            "any-authenticated-client",
            &config.key_ref,
            Some(&SignPurpose::Consensus)
        ));
        assert!(!policy.is_authorized(
            "any-authenticated-client",
            &KeyRef::new("remote", "validator-2", 2),
            Some(&SignPurpose::Consensus)
        ));
        // The default does not narrow purposes per identity: a purpose
        // outside the server whitelist is answered by the signing policy with
        // an authenticated permanent rejection, not by a transport-level 403.
        assert!(policy.is_authorized(
            "any-authenticated-client",
            &config.key_ref,
            Some(&SignPurpose::Transaction)
        ));
        assert!(!config.is_purpose_authorized(&SignPurpose::Transaction));
    }

    #[test]
    fn an_explicit_grant_restricts_purposes_per_identity() {
        let key = KeyRef::new("remote", "validator-1", 2);
        // An empty purpose list means "no per-identity narrowing".
        let unrestricted = ClientAuthorizationPolicy {
            grants: vec![ClientGrant::new(
                "neo-node-1",
                vec![key_selector(&key)],
                Vec::new(),
            )],
        };
        assert!(unrestricted.is_authorized("neo-node-1", &key, Some(&SignPurpose::Transaction)));
        // Listing purposes narrows the identity to exactly those purposes.
        let scoped = ClientAuthorizationPolicy {
            grants: vec![ClientGrant::new(
                "neo-node-1",
                vec![key_selector(&key)],
                vec![SignPurpose::Consensus],
            )],
        };
        assert!(scoped.is_authorized("neo-node-1", &key, Some(&SignPurpose::Consensus)));
        assert!(!scoped.is_authorized("neo-node-1", &key, Some(&SignPurpose::Transaction)));
    }

    #[test]
    fn a_policy_file_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("client-authorization.json");
        std::fs::write(
            &path,
            r#"{"grants":[{"common_name":"neo-node-1",
                "allowed_key_ids":["remote:validator-1"],
                "allowed_purposes":["consensus"]}]}"#,
        )
        .expect("write policy");
        let policy = ClientAuthorizationPolicy::from_json_file(&path).expect("policy");
        assert!(policy.is_authorized(
            "neo-node-1",
            &KeyRef::new("remote", "validator-1", 9),
            Some(&SignPurpose::Consensus)
        ));
        assert!(!policy.is_authorized(
            "neo-node-1",
            &KeyRef::new("remote", "validator-1", 9),
            Some(&SignPurpose::Transaction)
        ));
    }
}
