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
        "config/local.toml",
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
        let (settings, config) =
            load_config(&path, None).unwrap_or_else(|err| panic!("load {}: {err}", path.display()));
        validate_config(&config, settings.network)
            .unwrap_or_else(|err| panic!("validate {}: {err}", path.display()));

        assert!(config.rpc.enabled, "{relative} RPC enabled");
        assert!(config.indexer.enabled, "{relative} indexer enabled");
        assert!(
            config.indexer.backfill_on_startup,
            "{relative} indexer backfill"
        );
        match (&config.indexer.store_path, &config.indexer.path) {
            (Some(store_path), None) => {
                assert!(
                    !network_scoped_path(store_path, settings.network)
                        .as_os_str()
                        .is_empty(),
                    "{relative} indexer store path should not be empty"
                );
            }
            (None, Some(snapshot_path)) => {
                assert!(
                    network_scoped_path(snapshot_path, settings.network)
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("json")),
                    "{relative} indexer path should be a JSON snapshot file"
                );
            }
            (Some(_), Some(_)) => panic!("{relative} must not configure both indexer stores"),
            (None, None) => panic!("{relative} indexer durable path"),
        }

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

/// Service-provider presets should run the full public query stack a
/// NeoFura-style operator needs: hardened RPC, durable index services,
/// state proofs, and local health/metrics endpoints.
#[test]
fn shipped_service_provider_configs_enable_neofura_surface() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("neo-node has a workspace parent");
    let cases = [
        ("config/mainnet-service.toml", 0x334F_454E, 10332, "mainnet"),
        ("config/testnet-service.toml", 0x3554_334E, 20332, "testnet"),
    ];

    for (relative, expected_network, expected_rpc_port, data_scope) in cases {
        let path = workspace.join(relative);
        let (settings, config) =
            load_config(&path, None).unwrap_or_else(|err| panic!("load {}: {err}", path.display()));
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

        assert!(config.state_service.enabled, "{relative} state service");
        assert!(config.state_service.full_state, "{relative} full state");
        assert!(
            config.state_service.path.is_some(),
            "{relative} state store path"
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
BackfillOnStartup = true
"#,
    )
    .expect("parse indexer config");

    assert!(config.indexer.enabled);
    assert_eq!(
        config.indexer.store_path.as_deref(),
        Some(std::path::Path::new("Indexer_{0}"))
    );
    assert!(config.indexer.path.is_none());
    assert!(config.indexer.backfill_on_startup);
}
