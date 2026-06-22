use super::super::config::network_scoped_path;
use super::*;

/// A representative TOML config (mirroring the shipped
/// `neo_testnet_node.toml`) parses, derives the TestNet protocol
/// preset, and exposes the operational sections the daemon wires.
#[test]
fn load_config_parses_testnet_toml_and_derives_settings() {
    let toml = r#"
[network]
network_type = "TestNet"
network_magic = 0x3554334E

[storage]
backend = "rocksdb"
data_dir = "./data/testnet"
read_only = false
cache_size = 2048

[p2p]
port = 20333
enable_compression = true
min_desired_connections = 10
max_connections = 40
max_connections_per_address = 3
max_known_hashes = 1000
seed_nodes = ["seed1t5.neo.org:20333", "seed2t5.neo.org:20333"]

[rpc]
enabled = true
port = 20332
bind_address = "127.0.0.1"

# Optional operational sections must parse and validate.
[consensus]
enabled = false

[telemetry.metrics]
enabled = false
"#;
    let dir = std::env::temp_dir();
    let path = dir.join(format!("neo_node_cfg_test_{}.toml", std::process::id()));
    std::fs::write(&path, toml).expect("write temp config");

    let (settings, config) = load_config(&path, None).expect("load config");
    std::fs::remove_file(&path).ok();

    // TestNet preset derived from network_type, magic applied.
    assert_eq!(settings.network, 0x3554_334E);
    assert!(
        !settings.standby_committee.is_empty(),
        "preset seeds a committee"
    );

    // Operational sections the daemon wires.
    assert_eq!(config.storage.backend.as_deref(), Some("rocksdb"));
    assert_eq!(
        config.storage.data_dir.as_deref(),
        Some(std::path::Path::new("./data/testnet"))
    );
    assert_eq!(config.p2p.port, Some(20333));
    assert_eq!(config.p2p.seed_nodes.len(), 2);
    let channels = config.p2p.channels_config().expect("p2p channels");
    assert!(channels.enable_compression);
    assert_eq!(channels.min_desired_connections, 10);
    assert_eq!(channels.max_connections, 40);
    assert_eq!(channels.max_connections_per_address, 3);
    assert_eq!(channels.max_known_hashes, 1_000);
    assert!(config.rpc.enabled);
    assert_eq!(config.rpc.port, Some(20332));
    assert_eq!(config.rpc.bind_address.as_deref(), Some("127.0.0.1"));
    assert!(!config.telemetry.metrics.enabled);
}

/// Node TOML protocol knobs must affect the `ProtocolSettings` used by the
/// daemon; otherwise shipped `[blockchain]` / `[mempool]` sections look
/// meaningful while the node silently runs different consensus limits.
#[test]
fn load_config_applies_blockchain_and_mempool_protocol_overrides() {
    let toml = r#"
[network]
network_type = "TestNet"

[blockchain]
block_time = 1000
max_transactions_per_block = 123
max_valid_until_block_increment = 456
max_traceable_blocks = 789

[mempool]
max_transactions = 321
"#;
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "neo_node_protocol_overrides_{}.toml",
        std::process::id()
    ));
    std::fs::write(&path, toml).expect("write temp config");

    let (settings, _) = load_config(&path, None).expect("load config");
    std::fs::remove_file(&path).ok();

    assert_eq!(settings.milliseconds_per_block, 1_000);
    assert_eq!(settings.max_transactions_per_block, 123);
    assert_eq!(settings.max_valid_until_block_increment, 456);
    assert_eq!(settings.max_traceable_blocks, 789);
    assert_eq!(settings.memory_pool_max_transactions, 321);
}

/// Accept C#-style / older shipped TOML aliases so operational settings are
/// not lost when configs use names like `Engine`, `Port`, or `[dbft]`.
#[test]
fn node_config_accepts_csharp_style_operational_aliases() {
    let toml = r#"
[storage]
Engine = "rocksdb"
path = "./data/testnet"
ReadOnly = true

[p2p]
Port = 20333
EnableCompression = false
MinDesiredConnections = 2
MaxConnections = -1
MaxConnectionsPerAddress = 1
MaxKnownHashes = 77

[rpc]
Enabled = true
Port = 20332
BindAddress = "127.0.0.1"
AuthEnabled = true
RpcUser = "neo"
RpcPass = "secret"
EnableCors = false
MaxGasInvoke = 50000000
MaxIteratorResultItems = 321
DisabledMethods = ["openwallet"]
MaxBatchSize = 64

[dbft]
enabled = true
auto_start = true
private_key_hex = "012345"

[state_service]
Enabled = true
Path = "StateRoot"
FullState = true

[indexer]
Enabled = true
Path = "Indexer_{0}.json"
BackfillOnStartup = false
"#;
    let config: NodeConfig = toml::from_str(toml).expect("parses aliases");

    assert_eq!(config.storage.backend.as_deref(), Some("rocksdb"));
    assert_eq!(
        config.storage.data_directory(),
        Some(std::path::PathBuf::from("./data/testnet"))
    );
    assert!(config.storage.read_only);
    assert_eq!(config.p2p.port, Some(20333));
    let channels = config.p2p.channels_config().expect("p2p channels");
    assert!(!channels.enable_compression);
    assert_eq!(channels.min_desired_connections, 2);
    assert_eq!(channels.max_connections, usize::MAX);
    assert_eq!(channels.max_connections_per_address, 1);
    assert_eq!(channels.max_known_hashes, 77);
    let rpc = config.rpc.server_config(0x3554_334E).expect("rpc config");
    assert!(config.rpc.enabled);
    assert_eq!(rpc.network, 0x3554_334E);
    assert_eq!(rpc.port, 20332);
    assert_eq!(rpc.rpc_user, "neo");
    assert_eq!(rpc.rpc_pass, "secret");
    assert!(!rpc.enable_cors);
    assert_eq!(rpc.max_gas_invoke, 50_000_000);
    assert_eq!(rpc.max_iterator_result_items, 321);
    assert_eq!(rpc.disabled_methods, vec!["openwallet"]);
    assert_eq!(rpc.max_batch_size, 64);
    assert!(config.consensus.enabled);
    assert!(config.consensus.auto_start);
    assert_eq!(config.consensus.private_key_hex.as_deref(), Some("012345"));
    assert!(config.state_service.enabled);
    assert_eq!(
        config.state_service.path.as_deref(),
        Some(std::path::Path::new("StateRoot"))
    );
    assert!(config.state_service.full_state);
    assert!(config.indexer.enabled);
    assert_eq!(
        config.indexer.path.as_deref(),
        Some(std::path::Path::new("Indexer_{0}.json"))
    );
    assert!(config.indexer.store_path.is_none());
    assert!(!config.indexer.backfill_on_startup);
}

#[test]
fn storage_read_only_is_passed_to_rocksdb_open() {
    let temp = tempfile::tempdir().expect("temp RocksDB root");
    let missing_path = temp.path().join("missing-read-only-store");
    let config: NodeConfig = toml::from_str(&format!(
        r#"
[storage]
backend = "rocksdb"
data_dir = "{}"
read_only = true
"#,
        missing_path.display()
    ))
    .expect("parse read-only storage config");

    let err = match open_store(&config, None) {
        Ok(_) => panic!("read-only RocksDB should not create stores"),
        Err(err) => err,
    };
    let message = err.to_string();
    assert!(
        message.contains("failed to open RocksDB store"),
        "unexpected error: {message}"
    );
}

#[test]
fn rpc_section_maps_shipped_snake_case_fields_to_server_config() {
    let config: NodeConfig = toml::from_str(
        r#"
[rpc]
enabled = true
port = 10332
bind_address = "127.0.0.1"
auth_enabled = true
rpc_user = "neo"
rpc_pass = "change-me"
cors_enabled = false
max_gas_invoke = 50000000
max_iterator_results = 100
disabled_methods = ["openwallet"]
max_request_body_size = 1048576
max_batch_size = 32
session_enabled = true
session_expiration_time = 120
"#,
    )
    .expect("parse rpc config");

    let rpc = config.rpc.server_config(0x334F_454E).expect("rpc config");
    assert_eq!(rpc.network, 0x334F_454E);
    assert_eq!(rpc.port, 10332);
    assert_eq!(rpc.rpc_user, "neo");
    assert_eq!(rpc.rpc_pass, "change-me");
    assert!(!rpc.enable_cors);
    assert_eq!(rpc.max_gas_invoke, 50_000_000);
    assert_eq!(rpc.max_iterator_result_items, 100);
    assert_eq!(rpc.disabled_methods, vec!["openwallet"]);
    assert_eq!(rpc.max_request_body_size, 1_048_576);
    assert_eq!(rpc.max_batch_size, 32);
    assert!(rpc.session_enabled);
    assert_eq!(rpc.session_expiration_time, 120);
}

#[test]
fn network_scoped_path_formats_service_placeholders() {
    assert_eq!(
        network_scoped_path(Path::new("Data_MPT_{0}"), 0x004F_454Eu32),
        PathBuf::from("Data_MPT_004F454E")
    );
    assert_eq!(
        network_scoped_path(Path::new("StateRoot"), 0x004F_454Eu32),
        PathBuf::from("StateRoot")
    );
    assert_eq!(
        network_scoped_path(Path::new("Indexer_{0}.json"), 0x004F_454Eu32),
        PathBuf::from("Indexer_004F454E.json")
    );
    assert_eq!(
        network_scoped_path(Path::new("ApplicationLogs_{0}"), 0x004F_454Eu32),
        PathBuf::from("ApplicationLogs_004F454E")
    );
}

#[test]
fn p2p_channels_reject_invalid_negative_max_connections() {
    let config: NodeConfig = toml::from_str(
        r#"
[p2p]
max_connections = -2
"#,
    )
    .expect("parse config");

    let err = config
        .p2p
        .channels_config()
        .expect_err("rejects invalid max");
    assert!(err.to_string().contains("max_connections"));
}

/// The operator-facing presets checked into this repository should carry
/// Neo N3 v3.10.0 mainnet/testnet transaction limits explicitly.
#[test]
fn shipped_mainnet_and_testnet_configs_match_v3100_transaction_limits() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("neo-node has a workspace parent");
    let cases = [
        ("config/mainnet.toml", 200),
        ("config/mainnet-service.toml", 200),
        ("config/mainnet-stateroot.toml", 200),
        ("neo_mainnet_node.toml", 200),
        ("neo_production_node.toml", 200),
        ("config/testnet.toml", 5_000),
        ("config/testnet-service.toml", 5_000),
        ("neo_testnet_node.toml", 5_000),
    ];

    for (relative, expected) in cases {
        let path = workspace.join(relative);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        let config: NodeConfig =
            toml::from_str(&text).unwrap_or_else(|err| panic!("parse {}: {err}", path.display()));

        assert_eq!(
            config.blockchain.max_transactions_per_block,
            Some(expected),
            "{} must pin v3.10.0 MaxTransactionsPerBlock",
            relative
        );
    }
}

/// The public-network presets should mirror the Neo v3.10.0
/// `ApplicationConfiguration.P2P` channel defaults: compression enabled,
/// 10 desired peers, 40 max peers, 3 peers per address, and 1000 known
/// hashes. Local/private configs may intentionally override these.
#[test]
fn shipped_public_configs_match_v3100_p2p_channel_defaults() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("neo-node has a workspace parent");
    let cases = [
        ("config/mainnet.toml", Some(10333)),
        ("config/mainnet-service.toml", Some(10333)),
        ("config/mainnet-stateroot.toml", Some(10333)),
        ("neo_mainnet_node.toml", Some(10333)),
        ("neo_production_node.toml", Some(10333)),
        ("config/testnet.toml", Some(20333)),
        ("config/testnet-service.toml", Some(20333)),
        ("neo_testnet_node.toml", Some(20333)),
    ];

    for (relative, expected_port) in cases {
        let path = workspace.join(relative);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        let config: NodeConfig =
            toml::from_str(&text).unwrap_or_else(|err| panic!("parse {}: {err}", path.display()));
        let channels = config
            .p2p
            .channels_config()
            .unwrap_or_else(|err| panic!("build p2p channels for {}: {err}", path.display()));

        assert_eq!(config.p2p.port, expected_port, "{relative} P2P port");
        assert!(channels.enable_compression, "{relative} compression");
        assert_eq!(channels.min_desired_connections, 10, "{relative} min");
        assert_eq!(channels.max_connections, 40, "{relative} max");
        assert_eq!(
            channels.max_connections_per_address, 3,
            "{relative} per-address cap"
        );
        assert_eq!(channels.max_known_hashes, 1_000, "{relative} known hashes");
    }
}

/// A missing config file falls back to the built-in defaults (the
/// MainNet preset) rather than failing.
#[test]
fn load_config_missing_file_uses_defaults() {
    let path = PathBuf::from("/nonexistent/neo-node/definitely-missing.toml");
    let (settings, config) = load_config(&path, None).expect("defaults");
    assert_eq!(settings.network, ProtocolSettings::default().network);
    assert!(config.p2p.seed_nodes.is_empty());
    assert!(!config.rpc.enabled);
}

/// The `--network-magic` CLI override wins over the preset / config.
#[test]
fn load_config_magic_override_wins() {
    let path = PathBuf::from("/nonexistent/neo-node/missing.toml");
    let (settings, _) = load_config(&path, Some(0x1234_5678)).expect("override");
    assert_eq!(settings.network, 0x1234_5678);
}

/// Unknown / extra `[storage]` keys do not break parsing.
#[test]
fn node_config_ignores_unknown_keys() {
    let toml = r#"
[storage]
backend = "memory"
some_future_key = 42
"#;
    let config: NodeConfig = toml::from_str(toml).expect("tolerates unknown keys");
    assert_eq!(config.storage.backend.as_deref(), Some("memory"));
}

#[test]
fn node_cli_accepts_preflight_flags() {
    let cli = NodeCli::try_parse_from([
        "neo-node",
        "--config",
        "custom.toml",
        "--storage-path",
        "./data/custom",
        "--network-magic",
        "1234",
        "--check-all",
    ])
    .expect("preflight args parse");

    assert_eq!(cli.config, PathBuf::from("custom.toml"));
    assert_eq!(cli.storage_path, Some(PathBuf::from("./data/custom")));
    assert_eq!(cli.network_magic, Some(1234));
    assert!(cli.check_all);
    assert!(!cli.check_config);
    assert!(!cli.check_storage);
}

#[test]
fn validate_config_rejects_unknown_storage_backend() {
    let config: NodeConfig = toml::from_str(
        r#"
[storage]
backend = "rockdb"
"#,
    )
    .expect("parse config");

    let err = validate_config(&config, 0x3554_334E).expect_err("rejects typo");
    assert!(err.to_string().contains("unsupported [storage].backend"));
}

mod observability;
mod services;
