//! Neo Node configuration section struct definitions.
//!
//! This module contains all config struct definitions used by the Neo N3 node.

// Phase 3 S6d2: diagnostics config sank into neo-ai-agent; re-exported here
// for NodeConfig and the historical crate::config paths.
pub use neo_ai_agent::DiagnosticsSection;
#[cfg(test)]
pub use neo_ai_agent::ModelProviderSection;
use neo_primitives::UnhandledExceptionPolicy;
// Phase 3 S6c: PrivacySection sank into neo-tee::config; re-exported here
// (used intra-file by NodeConfig and re-exported via `pub use sections::*`) so
// the historical `crate::config::PrivacySection` path stays valid.
pub use neo_tee::config::PrivacySection;
use neo_wallets::signing::SignerServiceSection;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// High-level node configuration derived from the Neo CLI TOML files.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct NodeConfig {
    pub network: NetworkSection,
    pub p2p: P2PSection,
    pub storage: StorageSection,
    pub blockchain: BlockchainSection,
    /// Transaction execution engine selection. Defaults to the canonical VM.
    pub execution: ExecutionSection,
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
    /// Row cache size in MiB.
    ///
    /// Unset derives the row cache from the block cache (the historical
    /// behaviour), `0` disables the row cache, and any other value allocates
    /// exactly that many MiB for it.
    #[serde(alias = "RowCacheSize")]
    pub row_cache_size: Option<u32>,
    #[serde(alias = "Compression")]
    pub compression: Option<String>,
    #[serde(alias = "WriteBufferSize")]
    pub write_buffer_size: Option<u64>,
    #[serde(alias = "MaxOpenFiles")]
    pub max_open_files: Option<u32>,
    #[serde(alias = "ReadOnly")]
    pub read_only: Option<bool>,
    /// Speculative storage prefetching (`[storage.prefetch]`), off by default.
    #[serde(alias = "Prefetch")]
    pub prefetch: PrefetchSection,
}

/// Opt-in speculative prefetching, configured under `[storage.prefetch]`.
///
/// Every field is optional so an operator only states what they mean; an absent
/// section leaves prefetching disabled, which is the production default.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PrefetchSection {
    /// Master switch. `false` (the default) makes prefetching inert.
    #[serde(alias = "Enabled")]
    pub enabled: Option<bool>,
    /// Keys a single scan aims to prefetch.
    #[serde(alias = "Count")]
    pub count: Option<usize>,
    /// Minimum access-pattern confidence (0-100) before a scan may start.
    #[serde(alias = "ConfidenceThreshold")]
    pub confidence_threshold: Option<u8>,
    /// Minimum number of consecutive sequential observations before a scan.
    #[serde(alias = "MinObservations")]
    pub min_observations: Option<u32>,
    /// Per-scan count and byte ceilings.
    #[serde(alias = "Limits")]
    pub limits: PrefetchLimitsSection,
}

/// Per-scan ceilings for speculative prefetching (`[storage.prefetch.limits]`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PrefetchLimitsSection {
    /// Rows a single scan may read from the backing store.
    #[serde(alias = "MaxScanKeys")]
    pub max_scan_keys: Option<usize>,
    /// Keys a single scan may admit into the read cache.
    #[serde(alias = "MaxAdmitKeys")]
    pub max_admit_keys: Option<usize>,
    /// Value bytes a single scan may admit into the read cache.
    #[serde(alias = "MaxAdmitBytes")]
    pub max_admit_bytes: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct BlockchainSection {
    /// Block time in milliseconds (ms)
    pub block_time: Option<u64>,
    /// Maximum transactions per block
    pub max_transactions_per_block: Option<u32>,
    /// Maximum free transactions per block
    pub max_free_transactions_per_block: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExecutionSection {
    /// `canonical`, `auto`, or `verify`; invalid values are rejected at startup.
    #[serde(alias = "Mode")]
    pub mode: String,
    /// Enables observation-only persistence profiling.
    ///
    /// Profiling feeds the MPT stage and native hook telemetry families through
    /// `neo_mpt_stage_seconds_total` and `neo_native_hook_seconds_total`. It
    /// reads clocks around work the pipeline performs anyway and never changes
    /// execution results.
    #[serde(alias = "Profiling")]
    pub profiling: bool,
    /// Enables speculative parallel transaction execution.
    ///
    /// The scheduler executes a block's transactions on the Rayon pool against
    /// isolated overlays and then commits them in block order, re-executing any
    /// transaction whose read set a predecessor invalidated. Results, receipts,
    /// notifications and faults are identical to the serial loop by
    /// construction; only the wall-clock work moves. Disabled by default.
    #[serde(alias = "ParallelEnabled")]
    pub parallel_enabled: bool,
}

impl Default for ExecutionSection {
    fn default() -> Self {
        Self {
            mode: "canonical".to_string(),
            profiling: false,
            parallel_enabled: false,
        }
    }
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

/// Mempool configuration section
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct MempoolSection {
    /// Maximum transactions in mempool
    pub max_transactions: Option<usize>,
    /// Maximum transactions per sender
    pub max_transactions_per_sender: Option<usize>,
}
