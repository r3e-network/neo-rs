//! Command-line interface definitions for neo-node.

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "neo-node", about = "Neo N3 blockchain node daemon", version)]
pub struct NodeCli {
    /// Path to the TOML configuration file.
    #[arg(
        long,
        short = 'c',
        default_value = "neo_mainnet_node.toml",
        env = "NEO_CONFIG",
        value_name = "PATH"
    )]
    pub config: PathBuf,

    /// Overrides the configured storage path.
    #[arg(long, value_name = "PATH", env = "NEO_STORAGE")]
    pub storage: Option<PathBuf>,

    /// Overrides the storage backend (memory, rocksdb).
    #[arg(long, value_name = "BACKEND", env = "NEO_BACKEND")]
    pub backend: Option<String>,

    /// Open storage read-only (offline checks only).
    #[arg(long, env = "NEO_STORAGE_READONLY")]
    pub storage_read_only: bool,

    /// Overrides the network magic used during the P2P handshake.
    #[arg(long, value_name = "MAGIC", env = "NEO_NETWORK_MAGIC")]
    pub network_magic: Option<u32>,

    /// Overrides the P2P listening port.
    #[arg(long, value_name = "PORT", env = "NEO_LISTEN_PORT")]
    pub listen_port: Option<u16>,

    /// Replaces the configured seed nodes (comma separated).
    #[arg(
        long = "seed",
        value_delimiter = ',',
        value_name = "HOST:PORT",
        env = "NEO_SEED_NODES"
    )]
    pub seed_nodes: Vec<String>,

    /// Overrides the maximum number of concurrent connections.
    #[arg(long, value_name = "N", env = "NEO_MAX_CONNECTIONS")]
    pub max_connections: Option<usize>,

    /// Overrides the minimum desired number of peers.
    #[arg(long, value_name = "N", env = "NEO_MIN_CONNECTIONS")]
    pub min_connections: Option<usize>,

    /// Overrides the per-address connection cap.
    #[arg(long, value_name = "N", env = "NEO_MAX_CONNECTIONS_PER_ADDRESS")]
    pub max_connections_per_address: Option<usize>,

    /// Maximum broadcast history entries to retain in memory.
    #[arg(long, value_name = "N", env = "NEO_BROADCAST_HISTORY_LIMIT")]
    pub broadcast_history_limit: Option<usize>,

    /// Disables compression for outbound connections.
    #[arg(long, env = "NEO_DISABLE_COMPRESSION")]
    pub disable_compression: bool,

    /// Overrides the block time in seconds.
    #[arg(long, value_name = "SECONDS", env = "NEO_BLOCK_TIME")]
    pub block_time: Option<u64>,

    /// Run in daemon mode (no console output except errors).
    #[arg(long, short = 'd', env = "NEO_DAEMON")]
    pub daemon: bool,

    /// Override RPC bind address.
    #[arg(long, value_name = "ADDR", env = "NEO_RPC_BIND")]
    pub rpc_bind: Option<String>,

    /// Override RPC port.
    #[arg(long, value_name = "PORT", env = "NEO_RPC_PORT")]
    pub rpc_port: Option<u16>,

    /// Disable RPC CORS.
    #[arg(long, env = "NEO_RPC_DISABLE_CORS")]
    pub rpc_disable_cors: bool,

    /// Override RPC basic auth username.
    #[arg(long, value_name = "USER", env = "NEO_RPC_USER")]
    pub rpc_user: Option<String>,

    /// Override RPC basic auth password.
    #[arg(long, value_name = "PASS", env = "NEO_RPC_PASS")]
    pub rpc_pass: Option<String>,

    /// Override RPC TLS certificate path.
    #[arg(long, value_name = "PATH", env = "NEO_RPC_TLS_CERT")]
    pub rpc_tls_cert: Option<String>,

    /// Override RPC TLS certificate password.
    #[arg(long, value_name = "PASS", env = "NEO_RPC_TLS_PASS")]
    pub rpc_tls_cert_password: Option<String>,

    /// Override logging path.
    #[arg(long, value_name = "PATH", env = "NEO_LOG_PATH")]
    pub logging_path: Option<String>,

    /// Override logging level.
    #[arg(long, value_name = "LEVEL", env = "NEO_LOG_LEVEL")]
    pub logging_level: Option<String>,

    /// Override logging format.
    #[arg(long, value_name = "FORMAT", env = "NEO_LOG_FORMAT")]
    pub logging_format: Option<String>,

    /// Override RPC allowed CORS origins (comma-separated).
    #[arg(
        long,
        value_delimiter = ',',
        value_name = "ORIGIN",
        env = "NEO_RPC_ALLOW_ORIGINS"
    )]
    pub rpc_allow_origins: Vec<String>,

    /// Override RPC disabled methods (comma-separated).
    #[arg(
        long,
        value_delimiter = ',',
        value_name = "METHOD",
        env = "NEO_RPC_DISABLED_METHODS"
    )]
    pub rpc_disabled_methods: Vec<String>,

    /// Apply hardened RPC defaults (auth required, CORS disabled, common risky methods disabled).
    #[arg(long)]
    pub rpc_hardened: bool,

    /// Enable TEE (Trusted Execution Environment) mode for wallet and mempool protection.
    #[cfg(feature = "tee")]
    #[arg(long)]
    pub tee: bool,

    /// Path to store TEE sealed data.
    #[cfg(feature = "tee")]
    #[arg(long, value_name = "PATH", default_value = "./tee_data")]
    pub tee_data_path: PathBuf,

    /// TEE fair ordering policy (fcfs, batched, commit-reveal).
    #[cfg(feature = "tee")]
    #[arg(long, value_name = "POLICY", default_value = "batched")]
    pub tee_ordering_policy: String,

    /// Enable HSM (Hardware Security Module) mode for wallet signing.
    #[cfg(feature = "hsm")]
    #[arg(long)]
    pub hsm: bool,

    /// HSM device type (ledger, pkcs11, simulation).
    #[cfg(feature = "hsm")]
    #[arg(long, value_name = "DEVICE", default_value = "ledger")]
    pub hsm_device: String,

    /// PKCS#11 library path (required for pkcs11 device type).
    #[cfg(feature = "hsm")]
    #[arg(long, value_name = "PATH")]
    pub hsm_pkcs11_lib: Option<PathBuf>,

    /// HSM slot ID for PKCS#11 or Ledger device index.
    #[cfg(feature = "hsm")]
    #[arg(long, value_name = "SLOT", default_value = "0")]
    pub hsm_slot: u64,

    /// HSM key ID or derivation path (e.g., m/44'/888'/0'/0/0 for Ledger).
    #[cfg(feature = "hsm")]
    #[arg(long, value_name = "KEY_ID")]
    pub hsm_key_id: Option<String>,

    /// Skip interactive PIN prompt (for testing only, not recommended).
    #[cfg(feature = "hsm")]
    #[arg(long)]
    pub hsm_no_pin: bool,

    /// Validate configuration and exit without starting the node.
    #[arg(long)]
    pub check_config: bool,

    /// Validate storage backend connectivity and exit without starting the node.
    #[arg(long)]
    pub check_storage: bool,

    /// Run both config and storage checks, then exit.
    #[arg(long)]
    pub check_all: bool,

    /// Enable a lightweight health check server (HTTP on localhost) reporting readiness.
    #[arg(long, value_name = "PORT", env = "NEO_HEALTH_PORT")]
    pub health_port: Option<u16>,

    /// Fail healthz when header lag exceeds this value (blocks). Defaults to 20 when not set; use 0 to disable the check.
    #[arg(long, value_name = "BLOCKS", env = "NEO_HEALTH_MAX_HEADER_LAG")]
    pub health_max_header_lag: Option<u32>,

    /// Enable state root calculation and validation.
    #[arg(long, env = "NEO_STATE_ROOT", alias = "stateroot")]
    pub state_root: bool,

    /// Path to store state root data (defaults to StateRoot subdirectory of storage path).
    #[arg(long, value_name = "PATH", env = "NEO_STATE_ROOT_PATH")]
    pub state_root_path: Option<PathBuf>,

    /// Enable full state history tracking (keeps all historical state, increases storage usage).
    #[arg(long, env = "NEO_STATE_ROOT_FULL_STATE")]
    pub state_root_full_state: bool,

    /// Path to NEP-6 wallet file for validator mode.
    #[arg(long, value_name = "PATH", env = "NEO_WALLET")]
    pub wallet: Option<PathBuf>,

    /// Password for the NEP-6 wallet file.
    #[arg(long, value_name = "PASSWORD", env = "NEO_WALLET_PASSWORD")]
    pub wallet_password: Option<String>,
}
