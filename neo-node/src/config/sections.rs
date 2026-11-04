//! Neo Node configuration section struct definitions.
//!
//! This module contains all config struct definitions used by the Neo N3 node.

use neo_core::UnhandledExceptionPolicy;
use neo_core::signing::{SecurityLevel, SignPurpose};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use zeroize::Zeroizing;

/// High-level node configuration derived from the Neo CLI TOML files.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct NodeConfig {
    pub network: NetworkSection,
    pub p2p: P2PSection,
    pub storage: StorageSection,
    pub blockchain: BlockchainSection,
    pub rpc: RpcSection,
    pub application_logs: Option<ApplicationLogsSection>,
    pub state_service: Option<StateServiceSection>,
    pub tokens_tracker: Option<TokensTrackerSection>,
    pub oracle_service: Option<OracleServiceSection>,
    pub dbft: Option<DbftSection>,
    pub logging: LoggingSection,
    pub unlock_wallet: UnlockWalletSection,
    pub contracts: ContractsSection,
    pub plugins: PluginsSection,
    /// Consensus configuration section
    pub consensus: Option<ConsensusSection>,
    /// Telemetry configuration section
    pub telemetry: Option<TelemetrySection>,
    /// Mempool configuration section
    pub mempool: Option<MempoolSection>,
    /// Unified signing service configuration.
    pub signer_service: Option<SignerServiceSection>,
    /// Capability-limited node diagnostics configuration.
    pub diagnostics: Option<DiagnosticsSection>,
    /// Strictly opt-in encrypted-mempool privacy mode configuration.
    pub privacy: Option<PrivacySection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct NetworkSection {
    #[serde(alias = "NetworkType")]
    pub network_type: Option<String>,
    #[serde(alias = "Network")]
    pub network_magic: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct P2PSection {
    #[serde(alias = "Port", alias = "port")]
    pub listen_port: Option<u16>,
    #[serde(alias = "MinDesiredConnections")]
    pub min_desired_connections: Option<usize>,
    #[serde(alias = "MaxConnections")]
    pub max_connections: Option<usize>,
    #[serde(alias = "MaxConnectionsPerAddress")]
    pub max_connections_per_address: Option<usize>,
    #[serde(alias = "MaxKnownHashes")]
    pub max_known_hashes: Option<usize>,
    #[serde(alias = "BroadcastHistoryLimit")]
    pub broadcast_history_limit: Option<usize>,
    #[serde(alias = "EnableCompression")]
    pub enable_compression: Option<bool>,
    #[serde(alias = "SeedList")]
    pub seed_nodes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct StorageSection {
    #[serde(alias = "Path", alias = "data_dir")]
    pub path: Option<String>,
    #[serde(alias = "Engine")]
    pub backend: Option<String>,
    #[serde(alias = "CacheSize")]
    pub cache_size: Option<u64>,
    #[serde(alias = "Compression")]
    pub compression: Option<String>,
    #[serde(alias = "WriteBufferSize")]
    pub write_buffer_size: Option<u64>,
    #[serde(alias = "MaxOpenFiles")]
    pub max_open_files: Option<u32>,
    #[serde(alias = "ReadOnly")]
    pub read_only: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct BlockchainSection {
    /// Block time in milliseconds
    pub block_time: Option<u64>,
    /// Maximum transactions per block
    pub max_transactions_per_block: Option<u32>,
    /// Maximum free transactions per block
    pub max_free_transactions_per_block: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Default, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RpcSection {
    #[serde(alias = "Enabled")]
    pub enabled: bool,
    #[serde(alias = "BindAddress")]
    pub bind_address: Option<String>,
    #[serde(alias = "Port")]
    pub port: Option<u16>,
    #[serde(alias = "EnableCors")]
    pub cors_enabled: Option<bool>,
    #[serde(alias = "AllowOrigins")]
    pub allow_origins: Vec<String>,
    #[serde(alias = "MaxConcurrentConnections", alias = "MaxConnections")]
    pub max_connections: Option<usize>,
    #[serde(alias = "MaxRequestBodySize")]
    pub max_request_body_size: Option<usize>,
    #[serde(alias = "MaxGasInvoke")]
    pub max_gas_invoke: Option<f64>,
    #[serde(alias = "MaxFee")]
    pub max_fee: Option<f64>,
    #[serde(alias = "MaxIteratorResultItems", alias = "max_iterator_results")]
    pub max_iterator_result_items: Option<usize>,
    #[serde(alias = "MaxStackSize")]
    pub max_stack_size: Option<usize>,
    #[serde(alias = "KeepAliveTimeout")]
    pub keep_alive_timeout: Option<i32>,
    #[serde(alias = "RequestHeadersTimeout")]
    pub request_headers_timeout: Option<u64>,
    #[serde(alias = "AuthEnabled")]
    pub auth_enabled: bool,
    #[serde(alias = "SessionEnabled")]
    pub session_enabled: Option<bool>,
    #[serde(alias = "SessionExpirationTime")]
    pub session_expiration_time: Option<u64>,
    #[serde(alias = "FindStoragePageSize")]
    pub find_storage_page_size: Option<usize>,
    #[serde(alias = "UnhandledExceptionPolicy")]
    pub unhandled_exception_policy: Option<String>,
    #[serde(alias = "RpcUser")]
    pub rpc_user: Option<Zeroizing<String>>,
    #[serde(alias = "RpcPass")]
    pub rpc_pass: Option<Zeroizing<String>>,
    #[serde(alias = "SslCert")]
    pub tls_cert_file: Option<String>,
    #[serde(alias = "SslCertPassword")]
    pub tls_cert_password: Option<Zeroizing<String>>,
    #[serde(alias = "TrustedAuthorities")]
    pub trusted_authorities: Vec<String>,
    #[serde(alias = "DisabledMethods")]
    pub disabled_methods: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ApplicationLogsSection {
    #[serde(alias = "Enabled")]
    pub enabled: bool,
    #[serde(alias = "Path")]
    pub path: Option<String>,
    #[serde(alias = "Network")]
    pub network: Option<u32>,
    #[serde(alias = "MaxStackSize")]
    pub max_stack_size: Option<usize>,
    #[serde(alias = "Debug")]
    pub debug: Option<bool>,
    #[serde(alias = "UnhandledExceptionPolicy")]
    pub unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct StateServiceSection {
    #[serde(alias = "Enabled")]
    pub enabled: bool,
    #[serde(alias = "Path")]
    pub path: Option<String>,
    #[serde(alias = "FullState")]
    pub full_state: Option<bool>,
    #[serde(alias = "Network")]
    pub network: Option<u32>,
    #[serde(alias = "AutoVerify")]
    pub auto_verify: Option<bool>,
    #[serde(alias = "MaxFindResultItems")]
    pub max_find_result_items: Option<usize>,
    #[serde(alias = "UnhandledExceptionPolicy")]
    pub unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TokensTrackerSection {
    #[serde(alias = "Enabled")]
    pub enabled: bool,
    #[serde(alias = "DBPath")]
    pub db_path: Option<String>,
    #[serde(alias = "TrackHistory")]
    pub track_history: Option<bool>,
    #[serde(alias = "MaxResults")]
    pub max_results: Option<u32>,
    #[serde(alias = "Network")]
    pub network: Option<u32>,
    #[serde(alias = "EnabledTrackers")]
    pub enabled_trackers: Vec<String>,
    #[serde(alias = "UnhandledExceptionPolicy")]
    pub unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct OracleServiceSection {
    #[serde(alias = "Enabled")]
    pub enabled: bool,
    #[serde(alias = "Network")]
    pub network: Option<u32>,
    #[serde(alias = "Nodes")]
    pub nodes: Vec<String>,
    #[serde(alias = "MaxTaskTimeout")]
    pub max_task_timeout: Option<u64>,
    #[serde(alias = "MaxOracleTimeout")]
    pub max_oracle_timeout: Option<u64>,
    #[serde(alias = "AllowPrivateHost")]
    pub allow_private_host: Option<bool>,
    #[serde(alias = "AllowedContentTypes")]
    pub allowed_content_types: Vec<String>,
    #[serde(alias = "Https")]
    pub https: Option<OracleServiceHttpsSection>,
    #[serde(alias = "NeoFS")]
    pub neofs: Option<OracleServiceNeoFsSection>,
    #[serde(alias = "AutoStart")]
    pub auto_start: Option<bool>,
    #[serde(alias = "UnhandledExceptionPolicy")]
    pub unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct OracleServiceHttpsSection {
    #[serde(alias = "Timeout")]
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct OracleServiceNeoFsSection {
    #[serde(alias = "EndPoint")]
    pub endpoint: Option<String>,
    #[serde(alias = "Timeout")]
    pub timeout: Option<u64>,
    #[serde(alias = "BearerToken")]
    pub bearer_token: Option<String>,
    #[serde(alias = "BearerSignature")]
    pub bearer_signature: Option<String>,
    #[serde(alias = "BearerSignatureKey")]
    pub bearer_signature_key: Option<String>,
    #[serde(alias = "WalletConnect")]
    pub wallet_connect: Option<bool>,
    #[serde(alias = "AutoSignBearer")]
    pub auto_sign_bearer: Option<bool>,
    #[serde(alias = "UseGrpc")]
    pub use_grpc: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct DbftSection {
    #[serde(alias = "Enabled")]
    pub enabled: bool,
    #[serde(alias = "RecoveryLogs")]
    pub recovery_logs: Option<String>,
    #[serde(alias = "IgnoreRecoveryLogs")]
    pub ignore_recovery_logs: Option<bool>,
    #[serde(alias = "AutoStart")]
    pub auto_start: Option<bool>,
    #[serde(alias = "Network")]
    pub network: Option<u32>,
    #[serde(alias = "MaxBlockSize")]
    pub max_block_size: Option<u32>,
    #[serde(alias = "MaxBlockSystemFee")]
    pub max_block_system_fee: Option<i64>,
    #[serde(alias = "UnhandledExceptionPolicy")]
    pub unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DbftSettings {
    pub recovery_logs: String,
    pub ignore_recovery_logs: bool,
    pub auto_start: bool,
    pub network: u32,
    pub max_block_size: u32,
    pub max_block_system_fee: i64,
    pub exception_policy: UnhandledExceptionPolicy,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingSection {
    #[serde(alias = "Active")]
    pub active: bool,
    pub level: Option<String>,
    pub format: Option<String>,
    #[serde(alias = "ConsoleOutput")]
    pub console_output: bool,
    #[serde(alias = "FileEnabled")]
    pub file_enabled: bool,
    #[serde(alias = "Path", alias = "path")]
    pub file_path: Option<String>,
    pub max_file_size: Option<String>,
    pub max_files: Option<u32>,
}

impl Default for LoggingSection {
    fn default() -> Self {
        Self {
            active: true,
            level: Some("info".to_string()),
            format: None,
            console_output: true,
            file_enabled: false,
            file_path: Some("Logs".to_string()),
            max_file_size: None,
            max_files: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct UnlockWalletSection {
    #[serde(alias = "Path")]
    pub path: Option<String>,
    #[serde(alias = "Password")]
    pub password: Option<String>,
    #[serde(alias = "IsActive")]
    pub is_active: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ContractsSection {
    #[serde(alias = "NeoNameService")]
    pub neo_name_service: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginsSection {
    #[serde(alias = "DownloadUrl")]
    pub download_url: String,
    #[serde(alias = "Prerelease")]
    pub prerelease: bool,
    #[serde(alias = "Version")]
    pub version: Option<String>,
}

impl Default for PluginsSection {
    fn default() -> Self {
        Self {
            download_url: "https://api.github.com/repos/neo-project/neo/releases".to_string(),
            prerelease: false,
            version: None,
        }
    }
}

/// Consensus configuration section
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ConsensusSection {
    /// Whether consensus is enabled
    pub enabled: bool,
    /// Whether to auto-start consensus
    pub auto_start: Option<bool>,
}

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignerProviderKind {
    /// Local NEP-6 software wallet.
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

impl Default for SignerProviderKind {
    fn default() -> Self {
        Self::Nep6
    }
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

/// Strictly opt-in encrypted-mempool privacy mode (Phase A).
///
/// With `enabled = false` (the default, or when the section is absent) the
/// node behaves exactly as before: no content-encryption engine is installed,
/// no privacy RPC endpoint is registered, and consensus keeps its existing
/// proposal path. There is no auto-enable path.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PrivacySection {
    /// Master switch for privacy mode. Strictly opt-in; default off.
    pub enabled: bool,
    /// Content-encryption key encapsulation mode: `"committee_key"` or
    /// `"tee_sealed_key"`.
    pub encryption_mode: String,
    /// Committee epoch key, hex-encoded (exactly 32 bytes / 64 hex chars).
    /// Required when `encryption_mode = "committee_key"` and `enabled = true`.
    pub committee_epoch_key_hex: Option<String>,
    /// Committee epoch id bound to the configured epoch key. Defaults to 1.
    pub committee_epoch_id: Option<u64>,
}

impl Default for PrivacySection {
    fn default() -> Self {
        Self {
            enabled: false,
            encryption_mode: "tee_sealed_key".to_string(),
            committee_epoch_key_hex: None,
            committee_epoch_id: None,
        }
    }
}

/// Telemetry configuration section
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TelemetrySection {
    /// Metrics configuration
    pub metrics: Option<TelemetryMetricsSection>,
}

/// Telemetry metrics configuration
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TelemetryMetricsSection {
    /// Whether metrics are enabled
    pub enabled: bool,
    /// Metrics port
    pub port: Option<u16>,
    /// Metrics bind address
    pub bind_address: Option<String>,
}

/// Capability-limited node diagnostics and bounded auto-action settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiagnosticsSection {
    /// Enables the diagnostic side-channel and worker.
    pub enabled: bool,
    /// Emergency switch that disables all operational actions.
    pub kill_switch: bool,
    /// Evaluates and audits actions without executing them.
    pub observe_only: bool,
    /// Enables structured action and worker audit events.
    pub audit_enabled: bool,
    /// Durable hash-chained JSONL audit path. Required when diagnostics are enabled.
    pub audit_path: String,
    /// Bounded evidence queue capacity.
    pub evidence_queue_capacity: usize,
    /// Base node evidence sampling interval.
    pub sampling_interval_ms: u64,
    /// Maximum age considered fresh by health reporting.
    pub event_freshness_ms: u64,
    /// Maximum independent mempool quarantine records.
    pub quarantine_max_entries: usize,
    /// Maximum quarantine lifetime, capped at 30 seconds.
    pub quarantine_ttl_ms: u64,
    /// Explicitly enables bounded local transaction quarantine.
    pub allow_transaction_quarantine: bool,
    /// Maximum mempool entries scanned for one local proposal.
    pub proposal_scan_budget: usize,
    /// Deterministic quarantine rule version.
    pub rule_version: String,
    /// Enables otherwise disabled consensus restart policy evaluation.
    pub allow_consensus_restart: bool,
    /// Enables the restricted probe interface. It remains fail-closed without containment.
    pub restricted_probe_enabled: bool,
    /// Absolute path to the fixed `neo-probe` worker executable.
    pub restricted_probe_worker_path: String,
    /// Maximum number of queued probe requests.
    pub restricted_probe_queue_capacity: usize,
    /// Number of probe workers. Only one is permitted.
    pub restricted_probe_concurrency: usize,
    /// Maximum duration of one replay.
    pub restricted_probe_run_timeout_ms: u64,
    /// Maximum duration of the complete deterministic replay suite.
    pub restricted_probe_suite_timeout_ms: u64,
    /// Exact worker process memory ceiling.
    pub restricted_probe_memory_limit_bytes: u64,
    /// Maximum request or response frame size.
    pub restricted_probe_frame_limit_bytes: usize,
}

impl Default for DiagnosticsSection {
    fn default() -> Self {
        Self {
            enabled: false,
            kill_switch: false,
            observe_only: true,
            audit_enabled: true,
            audit_path: "data/diagnostics-audit.jsonl".to_string(),
            evidence_queue_capacity: 1_024,
            sampling_interval_ms: 1_000,
            event_freshness_ms: 5_000,
            quarantine_max_entries: 1_024,
            quarantine_ttl_ms: 30_000,
            allow_transaction_quarantine: false,
            proposal_scan_budget: 2_048,
            rule_version: "node-diagnostics-v1".to_string(),
            allow_consensus_restart: false,
            restricted_probe_enabled: false,
            restricted_probe_worker_path: "neo-probe".to_string(),
            restricted_probe_queue_capacity: 16,
            restricted_probe_concurrency: 1,
            restricted_probe_run_timeout_ms: 3_000,
            restricted_probe_suite_timeout_ms: 7_000,
            restricted_probe_memory_limit_bytes: 256 * 1024 * 1024,
            restricted_probe_frame_limit_bytes: 512 * 1024,
        }
    }
}

/// Mempool configuration section
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct MempoolSection {
    /// Maximum transactions in mempool
    pub max_transactions: Option<usize>,
    /// Maximum transactions per sender
    pub max_transactions_per_sender: Option<usize>,
}
