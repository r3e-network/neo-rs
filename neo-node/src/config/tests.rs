use super::*;
use neo_core::protocol_settings::ProtocolSettings;
use std::sync::Mutex;
use std::{env, fs};
use tempfile::TempDir;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn set_env_var<K: AsRef<std::ffi::OsStr>, V: AsRef<std::ffi::OsStr>>(key: K, value: V) {
    // SAFETY: Tests serialize access around this process-global variable and
    // restore it before releasing the lock.
    #[allow(unused_unsafe)]
    unsafe {
        env::set_var(key, value);
    }
}

fn remove_env_var<K: AsRef<std::ffi::OsStr>>(key: K) {
    // SAFETY: Tests serialize access around this process-global variable and
    // restore it before releasing the lock.
    #[allow(unused_unsafe)]
    unsafe {
        env::remove_var(key);
    }
}

#[test]
fn rejects_unknown_fields_in_known_table() {
    let contents = r#"
        [network]
        network_type = "MainNet"
        unexpected = 1
    "#;
    let err = toml::from_str::<NodeConfig>(contents).expect_err("should reject unknown field");
    let msg = err.to_string().to_ascii_lowercase();
    assert!(
        msg.contains("unknown field") || msg.contains("unknown"),
        "unexpected error message: {msg}"
    );
}

#[test]
fn rejects_unknown_tables() {
    let contents = r#"
        [network]
        network_type = "MainNet"

        [extra]
        foo = "bar"
    "#;
    let err = toml::from_str::<NodeConfig>(contents).expect_err("should reject unknown table");
    let msg = err.to_string().to_ascii_lowercase();
    assert!(
        msg.contains("unknown field") || msg.contains("extra"),
        "unexpected error message: {msg}"
    );
}

#[test]
fn blockchain_block_time_is_milliseconds() {
    let mut config = NodeConfig::default();
    config.blockchain.block_time = Some(15_000);

    let settings = config.protocol_settings();
    assert_eq!(settings.milliseconds_per_block, 15_000);
}

#[test]
fn blockchain_max_transactions_override_applies_to_protocol_settings() {
    let mut config = NodeConfig::default();
    config.network.network_type = Some("TestNet".to_string());
    config.blockchain.max_transactions_per_block = Some(5_000);

    let settings = config.protocol_settings();
    assert_eq!(settings.max_transactions_per_block, 5_000);
}

#[test]
fn mempool_max_transactions_override_applies_to_protocol_settings() {
    let mut config = NodeConfig::default();
    config.network.network_type = Some("MainNet".to_string());
    config.mempool = Some(MempoolSection {
        max_transactions: Some(12_345),
        max_transactions_per_sender: None,
    });

    let settings = config.protocol_settings();
    assert_eq!(settings.memory_pool_max_transactions, 12_345);
}

#[test]
fn writes_rpc_config_with_restricted_permissions() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let tmp = TempDir::new().expect("temp dir");
    set_env_var("NEO_PLUGINS_DIR", tmp.path());

    let mut config = NodeConfig::default();
    config.rpc.enabled = true;
    config.rpc.port = Some(12345);

    let settings = ProtocolSettings::mainnet();
    let path = config
        .write_rpc_server_plugin_config(&settings)
        .expect("write rpc config")
        .expect("path returned");

    assert!(
        path.starts_with(tmp.path()),
        "rpc config should be written under NEO_PLUGINS_DIR"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(&path).expect("metadata");
        assert_eq!(metadata.mode() & 0o777, 0o600);
    }

    let contents = fs::read_to_string(&path).expect("contents");
    assert!(
        contents.contains("\"Servers\""),
        "config should contain Servers array"
    );

    remove_env_var("NEO_PLUGINS_DIR");
}

#[test]
fn bundled_mainnet_config_parses() {
    let cfg: NodeConfig = toml::from_str(include_str!("../../neo_mainnet_node.toml"))
        .expect("mainnet config should parse");
    assert_eq!(cfg.network.network_type.as_deref(), Some("MainNet"));
}

#[test]
fn bundled_testnet_config_parses() {
    let cfg: NodeConfig = toml::from_str(include_str!("../../neo_testnet_node.toml"))
        .expect("testnet config should parse");
    assert_eq!(cfg.network.network_type.as_deref(), Some("TestNet"));
}

#[test]
fn bundled_production_config_parses() {
    toml::from_str::<NodeConfig>(include_str!("../../neo_production_node.toml"))
        .expect("production template should parse");
}
