use super::*;
use neo_protocol::ProtocolSettings;
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
fn execution_profiling_is_opt_in_and_matches_the_legacy_alias() {
    let default = NodeConfig::default();
    assert_eq!(default.execution.mode, "canonical");
    assert!(!default.execution.profiling);

    let parsed = toml::from_str::<NodeConfig>(
        r#"
        [execution]
        Mode = "verify"
        Profiling = true
    "#,
    )
    .expect("profiling alias parses");
    assert_eq!(parsed.execution.mode, "verify");
    assert!(parsed.execution.profiling);

    let canonical = toml::from_str::<NodeConfig>(
        r#"
        [execution]
        profiling = true
    "#,
    )
    .expect("lowercase key parses");
    assert!(canonical.execution.profiling);
    assert_eq!(canonical.execution.mode, "canonical");
}

#[test]
fn parallel_execution_is_off_by_default_and_matches_the_legacy_alias() {
    let default = NodeConfig::default();
    assert!(!default.execution.parallel_enabled);

    let alias = toml::from_str::<NodeConfig>(
        r#"
        [execution]
        ParallelEnabled = true
    "#,
    )
    .expect("parallel alias parses");
    assert!(alias.execution.parallel_enabled);

    let canonical = toml::from_str::<NodeConfig>(
        r#"
        [execution]
        parallel_enabled = true
    "#,
    )
    .expect("lowercase key parses");
    assert!(canonical.execution.parallel_enabled);
    assert!(!canonical.execution.profiling);
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
    let cfg: NodeConfig = toml::from_str(include_str!("../../../neo_mainnet_node.toml"))
        .expect("mainnet config should parse");
    assert_eq!(cfg.network.network_type.as_deref(), Some("MainNet"));
}

#[test]
fn bundled_testnet_config_parses() {
    let cfg: NodeConfig = toml::from_str(include_str!("../../../neo_testnet_node.toml"))
        .expect("testnet config should parse");
    assert_eq!(cfg.network.network_type.as_deref(), Some("TestNet"));
}

#[test]
fn bundled_production_config_parses() {
    toml::from_str::<NodeConfig>(include_str!("../../../neo_production_node.toml"))
        .expect("production template should parse");
}

// --- [storage.prefetch] ---------------------------------------------------

/// A node config with no `[storage.prefetch]` table must keep prefetching off.
#[test]
fn storage_prefetch_absent_is_disabled() {
    let cfg: NodeConfig = toml::from_str(
        r#"
        [network]
        network_type = "MainNet"

        [storage]
        path = "Data_Test"
        "#,
    )
    .expect("config without a prefetch table should parse");

    let prefetch = cfg.storage_prefetch();
    assert!(!prefetch.enabled, "prefetch must default to off");
    assert!(!prefetch.is_active(), "disabled prefetch must be inert");
    assert!(
        !cfg.storage_prefetch_is_effective(),
        "absent section must not report an effective configuration"
    );
    assert_eq!(
        prefetch,
        neo_storage::persistence::data_cache::PrefetchConfig::default()
    );
}

/// Explicitly disabling the section behaves exactly like leaving it out.
#[test]
fn storage_prefetch_explicitly_disabled_is_inert() {
    let cfg: NodeConfig = toml::from_str(
        r#"
        [storage]
        path = "Data_Test"

        [storage.prefetch]
        enabled = false
        count = 16
        confidence_threshold = 90
        "#,
    )
    .expect("disabled prefetch table should parse");

    let prefetch = cfg.storage_prefetch();
    assert!(!prefetch.enabled);
    assert!(!prefetch.is_active());
    // The tuning values are still readable so an operator can stage a rollout.
    assert_eq!(prefetch.count, 16);
    assert_eq!(prefetch.confidence_threshold, 90);
}

/// The production opt-in form parses into the storage configuration verbatim.
#[test]
fn storage_prefetch_section_parses_production_values() {
    let cfg: NodeConfig = toml::from_str(
        r#"
        [storage]
        path = "Data_Test"

        [storage.prefetch]
        enabled = true
        count = 12
        confidence_threshold = 75
        min_observations = 6

        [storage.prefetch.limits]
        max_scan_keys = 96
        max_admit_keys = 10
        max_admit_bytes = 131072
        "#,
    )
    .expect("opt-in prefetch table should parse");

    let prefetch = cfg.storage_prefetch();
    assert!(prefetch.enabled);
    assert!(prefetch.is_active());
    assert!(cfg.storage_prefetch_is_effective());
    assert_eq!(prefetch.count, 12);
    assert_eq!(prefetch.confidence_threshold, 75);
    assert_eq!(prefetch.min_observations, 6);
    assert_eq!(prefetch.limits.max_scan_keys, 96);
    assert_eq!(prefetch.limits.max_admit_keys, 10);
    assert_eq!(prefetch.limits.max_admit_bytes, 131_072);
}

/// Unset keys inside an enabled section fall back to the documented defaults.
#[test]
fn storage_prefetch_enabled_without_tuning_uses_defaults() {
    let cfg: NodeConfig = toml::from_str(
        r#"
        [storage.prefetch]
        enabled = true
        "#,
    )
    .expect("minimal prefetch table should parse");

    let prefetch = cfg.storage_prefetch();
    let defaults = neo_storage::persistence::data_cache::PrefetchConfig::default();
    assert!(prefetch.enabled);
    assert!(prefetch.is_active());
    assert_eq!(prefetch.count, defaults.count);
    assert_eq!(prefetch.confidence_threshold, defaults.confidence_threshold);
    assert_eq!(prefetch.min_observations, defaults.min_observations);
    assert_eq!(prefetch.limits, defaults.limits);
}

/// Operator ceilings are clamped into the storage crate's hard bounds.
#[test]
fn storage_prefetch_limits_are_clamped() {
    use neo_storage::persistence::data_cache::PrefetchLimits;

    let cfg: NodeConfig = toml::from_str(
        r#"
        [storage.prefetch]
        enabled = true

        [storage.prefetch.limits]
        max_scan_keys = 1000000
        max_admit_keys = 1000000
        max_admit_bytes = 1000000000
        "#,
    )
    .expect("oversized prefetch limits should still parse");

    let prefetch = cfg.storage_prefetch();
    assert_eq!(
        prefetch.limits.max_scan_keys,
        PrefetchLimits::CEILING_SCAN_KEYS
    );
    assert_eq!(
        prefetch.limits.max_admit_keys,
        PrefetchLimits::CEILING_ADMIT_KEYS
    );
    assert_eq!(
        prefetch.limits.max_admit_bytes,
        PrefetchLimits::CEILING_ADMIT_BYTES
    );
}

/// A zero ceiling switches the feature off instead of enabling an empty scan.
#[test]
fn storage_prefetch_zero_ceiling_is_not_active() {
    let cfg: NodeConfig = toml::from_str(
        r#"
        [storage.prefetch]
        enabled = true
        count = 0
        "#,
    )
    .expect("zero count should parse");

    assert!(cfg.storage_prefetch().enabled);
    assert!(!cfg.storage_prefetch_is_effective());
}

/// Unknown keys inside the prefetch table are rejected, like every other section.
#[test]
fn storage_prefetch_rejects_unknown_fields() {
    let err = toml::from_str::<NodeConfig>(
        r#"
        [storage.prefetch]
        enabled = true
        magic = 1
        "#,
    )
    .expect_err("unknown prefetch field should be rejected");
    let msg = err.to_string().to_ascii_lowercase();
    assert!(msg.contains("unknown"), "unexpected error message: {msg}");
}

/// The bundled templates must keep parsing and must not switch prefetch on.
#[test]
fn bundled_configs_keep_prefetch_disabled() {
    for (name, contents) in [
        ("mainnet", include_str!("../../../neo_mainnet_node.toml")),
        ("testnet", include_str!("../../../neo_testnet_node.toml")),
        (
            "production",
            include_str!("../../../neo_production_node.toml"),
        ),
    ] {
        let cfg: NodeConfig = toml::from_str(contents)
            .unwrap_or_else(|err| panic!("{name} config should parse: {err}"));
        assert!(
            !cfg.storage_prefetch().enabled,
            "{name} template must ship with prefetch disabled"
        );
    }
}

// --- [storage] row cache --------------------------------------------------

const MIB: usize = 1024 * 1024;

/// An unset `row_cache_size` keeps deriving the row cache from the block cache.
#[test]
fn storage_row_cache_absent_keeps_derived_default() {
    let cfg: NodeConfig = toml::from_str(
        r#"
        [storage]
        path = "Data_Test"
        cache_size = 1024
        "#,
    )
    .expect("storage table without row_cache_size should parse");

    let storage = cfg.storage_config();
    assert_eq!(storage.cache_size, Some(1024 * MIB));
    assert_eq!(
        storage.row_cache_size_bytes, None,
        "an absent knob must keep the derived row cache"
    );
}

/// `row_cache_size = 0` is the documented way to disable the row cache.
#[test]
fn storage_row_cache_zero_disables_the_cache() {
    let cfg: NodeConfig = toml::from_str(
        r#"
        [storage]
        row_cache_size = 0
        "#,
    )
    .expect("a zero row cache should parse");

    assert_eq!(cfg.storage_config().row_cache_size_bytes, Some(0));
}

/// An explicit size is converted from MiB and forwarded verbatim.
#[test]
fn storage_row_cache_explicit_size_is_forwarded_in_bytes() {
    let cfg: NodeConfig = toml::from_str(
        r#"
        [storage]
        row_cache_size = 256
        "#,
    )
    .expect("an explicit row cache should parse");

    assert_eq!(cfg.storage_config().row_cache_size_bytes, Some(256 * MIB));
}

/// The Neo CLI spelling is accepted as an alias, like every other storage key.
#[test]
fn storage_row_cache_accepts_pascal_case_alias() {
    let cfg: NodeConfig = toml::from_str(
        r#"
        [storage]
        RowCacheSize = 384
        "#,
    )
    .expect("the PascalCase alias should parse");

    assert_eq!(cfg.storage_config().row_cache_size_bytes, Some(384 * MIB));
}

/// A misspelled key must still fail closed instead of being silently ignored.
#[test]
fn storage_rejects_unknown_row_cache_field() {
    let err = toml::from_str::<NodeConfig>(
        r#"
        [storage]
        row_cache_sizes = 256
        "#,
    )
    .expect_err("a misspelled row cache key should be rejected");
    let msg = err.to_string().to_ascii_lowercase();
    assert!(msg.contains("unknown"), "unexpected error message: {msg}");
}
