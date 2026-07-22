use super::*;

#[test]
fn plugin_settings_apply_shared_network_path_resolution() {
    let network = 0x004F_454E;
    let config: NodeConfig = toml::from_str(
        r#"
[application_logs]
enabled = true
Path = "ApplicationLogs_{0}"
MaxStackSize = 42
Debug = true

[tokens_tracker]
enabled = true
DBPath = "Tokens_{0}"
Network = 0x004F454E
TrackHistory = false
MaxResults = 7
EnabledTrackers = [" nep-17 ", "", "Nep-11"]
"#,
    )
    .expect("parse plugin config");

    let logs = config.application_logs.settings(network);
    assert!(logs.enabled);
    assert_eq!(logs.network, network);
    assert_eq!(logs.path, "ApplicationLogs_004F454E");
    assert_eq!(logs.max_stack_size, 42);
    assert!(logs.debug);

    let tracker = config.tokens_tracker.settings(0x3554_334E);
    assert_eq!(tracker.network, network);
    assert_eq!(tracker.db_path, "Tokens_004F454E");
    assert!(!tracker.track_history);
    assert_eq!(tracker.max_results, 7);
    assert_eq!(
        tracker.enabled_trackers,
        vec!["NEP-17".to_string(), "NEP-11".to_string()]
    );
}

#[test]
fn plugin_settings_preserve_plugin_default_paths() {
    let network = 0x004F_454E;
    let config: NodeConfig = toml::from_str(
        r#"
[application_logs]
enabled = true

[tokens_tracker]
enabled = true
"#,
    )
    .expect("parse plugin config");

    let logs = config.application_logs.settings(network);
    assert_eq!(logs.path, "ApplicationLogs_004F454E");

    let tracker = config.tokens_tracker.settings(network);
    assert_eq!(tracker.db_path, "TokenBalanceData");
    assert_eq!(
        tracker.enabled_trackers,
        vec!["NEP-17".to_string(), "NEP-11".to_string()]
    );
}

/// Operator-facing presets should enable the read-side service stack with
/// durable paths so a stock node can expose RPC, indexer, application-log,
/// and token-tracker services without hand wiring.
#[test]
fn shipped_node_configs_enable_durable_rpc_service_stack() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("neo-node has a workspace parent");
    let cases = [
        "config/mainnet.toml",
        "config/mainnet-service.toml",
        "config/mainnet-stateroot.toml",
        "config/testnet.toml",
        "config/testnet-service.toml",
        "neo_mainnet_node.toml",
        "neo_production_node.toml",
        "neo_testnet_node.toml",
    ];

    for relative in cases {
        let path = workspace.join(relative);
        let (chain_spec, config) =
            load_config(&path, None).unwrap_or_else(|err| panic!("load {}: {err}", path.display()));
        let settings = chain_spec.protocol_settings();
        validate_config(&config, settings.network)
            .unwrap_or_else(|err| panic!("validate {}: {err}", path.display()));

        assert!(config.rpc.enabled, "{relative} RPC enabled");
        assert!(config.indexer.enabled, "{relative} indexer enabled");
        let store_path = config
            .indexer
            .store_path
            .as_ref()
            .unwrap_or_else(|| panic!("{relative} must configure an indexer service store"));
        assert!(
            !network_scoped_path(store_path, settings.network)
                .as_os_str()
                .is_empty(),
            "{relative} indexer store path should not be empty"
        );

        assert!(
            config.application_logs.enabled,
            "{relative} application logs enabled"
        );
        assert!(
            !config
                .application_logs
                .settings(settings.network)
                .path
                .is_empty(),
            "{relative} application logs path"
        );
        assert!(config.tokens_tracker.enabled, "{relative} tokens tracker");
        assert!(
            !config
                .tokens_tracker
                .settings(settings.network)
                .db_path
                .is_empty(),
            "{relative} tokens tracker path"
        );
    }
}

#[test]
fn shipped_local_config_selects_canonical_testnet_identity() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("neo-node has a workspace parent");
    let path = workspace.join("config/local.toml");

    let (chain_spec, config) = load_config(&path, None).expect("local config must load");
    let settings = chain_spec.protocol_settings();
    assert_eq!(
        chain_spec.identity().network_type(),
        neo_config::NetworkType::TestNet
    );
    assert_eq!(settings.network, 0x3554_334E);
    validate_config(&config, settings.network)
        .expect("local config must satisfy canonical TestNet rules");
    assert_eq!(settings.milliseconds_per_block, 15_000);
    assert!(settings.max_transactions_per_block > 0);
}

/// Service-provider presets prepare the full public query stack a NeoFura-style
/// operator needs while retaining the explicit runtime gate for state proofs.
#[test]
fn shipped_service_provider_configs_prepare_neofura_surface_with_explicit_stateroot_gate() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("neo-node has a workspace parent");
    let cases = [
        ("config/mainnet-service.toml", 0x334F_454E, 10332, "mainnet"),
        ("config/testnet-service.toml", 0x3554_334E, 20332, "testnet"),
    ];

    for (relative, expected_network, expected_rpc_port, data_scope) in cases {
        let path = workspace.join(relative);
        let (chain_spec, config) =
            load_config(&path, None).unwrap_or_else(|err| panic!("load {}: {err}", path.display()));
        let settings = chain_spec.protocol_settings();
        validate_config(&config, settings.network)
            .unwrap_or_else(|err| panic!("validate {}: {err}", path.display()));

        assert_eq!(settings.network, expected_network, "{relative} network");
        assert!(
            config
                .storage
                .data_directory()
                .as_deref()
                .is_some_and(|path| path.ends_with(data_scope)),
            "{relative} storage path should be scoped to {data_scope}"
        );

        let rpc = config
            .rpc
            .server_config(settings.network)
            .unwrap_or_else(|err| panic!("rpc config for {relative}: {err}"));
        assert!(config.rpc.enabled, "{relative} RPC enabled");
        assert_eq!(rpc.port, expected_rpc_port, "{relative} RPC port");
        assert!(rpc.max_requests_per_second > 0, "{relative} RPC rate limit");
        assert!(rpc.rate_limit_burst > 0, "{relative} RPC burst limit");
        assert!(
            rpc.max_batch_size > 0,
            "{relative} RPC batch limit should be explicit"
        );

        assert!(
            !config.state_service.enabled,
            "{relative} StateRoot must require --enable-stateroot"
        );
        assert!(config.state_service.full_state, "{relative} full state");
        assert!(
            !config.state_service.defer_full_state_finalization,
            "{relative} keeps C#-compatible eager full-state finalization by default"
        );
        assert!(
            config.state_service.path.is_none(),
            "{relative} StateService should use the coordinated MDBX namespace"
        );
        assert!(
            config.indexer.store_path.is_some(),
            "{relative} service indexer store"
        );
        assert!(
            config.application_logs.enabled,
            "{relative} application logs"
        );
        assert!(config.tokens_tracker.enabled, "{relative} tokens tracker");
        assert!(
            config.telemetry.metrics.enabled,
            "{relative} metrics endpoint"
        );
        assert_eq!(
            config.telemetry.metrics.endpoint_path(),
            "/metrics",
            "{relative} metrics path"
        );
        assert_eq!(
            config.logging.format.as_deref(),
            Some("json"),
            "{relative} machine-readable logs"
        );
        assert!(
            config.logging.file_path.is_some(),
            "{relative} rotated log file"
        );
    }
}

#[test]
fn shipped_service_provider_configs_show_observability_provider_examples() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("neo-node has a workspace parent");
    let cases = ["config/mainnet-service.toml", "config/testnet-service.toml"];

    for relative in cases {
        let path = workspace.join(relative);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));

        assert!(
            text.contains("kind = \"better_stack_logs\""),
            "{relative} should show Better Stack log reporting"
        );
        assert!(
            text.contains("kind = \"google_error_reporting\""),
            "{relative} should show Google Error Reporting"
        );
        assert!(
            text.contains("kind = \"sentry\""),
            "{relative} should show Sentry error reporting"
        );
        assert!(
            text.contains("[observability.error_endpoints.headers_env]"),
            "{relative} should keep Sentry auth outside TOML secrets"
        );
        assert!(
            text.contains("X-Sentry-Auth = \"SENTRY_AUTH_HEADER\""),
            "{relative} should show Sentry header authentication via env"
        );
        assert!(
            !text.contains("sentry_key=your-public-key"),
            "{relative} should not encourage inline Sentry secrets"
        );
    }
}

#[test]
fn indexer_section_parses_service_store_aliases() {
    let config: NodeConfig = toml::from_str(
        r#"
[indexer]
Enabled = true
DBPath = "Indexer_{0}"
"#,
    )
    .expect("parse indexer config");

    assert!(config.indexer.enabled);
    assert_eq!(
        config.indexer.store_path.as_deref(),
        Some(std::path::Path::new("Indexer_{0}"))
    );
}

#[test]
fn indexer_section_rejects_removed_legacy_options() {
    for removed_option in [
        r#"
[indexer]
enabled = true
path = "Indexer.json"
"#,
        r#"
[indexer]
enabled = true
backfill_on_startup = false
"#,
    ] {
        let error = toml::from_str::<NodeConfig>(removed_option)
            .expect_err("removed indexer options must not fail open to memory");

        assert!(error.to_string().contains("unknown field"), "{error}");
    }
}
