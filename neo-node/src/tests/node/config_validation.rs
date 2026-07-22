use std::path::Path;

use super::*;
use neo_storage::persistence::{Store, StoreSnapshot, WriteStore};

#[test]
fn validate_config_rejects_incomplete_rpc_auth() {
    let config: NodeConfig = toml::from_str(
        r#"
[rpc]
enabled = true
auth_enabled = true
rpc_user = "neo"
"#,
    )
    .expect("parse config");

    let err = validate_config(&config, 0x3554_334E).expect_err("missing rpc password fails");
    assert!(err.to_string().contains("auth_enabled requires"));
}

#[test]
fn validate_config_requires_durable_writable_storage_for_static_files() {
    let mut ephemeral = NodeConfig::default();
    ephemeral.storage.backend = Some("memory".to_string());
    ephemeral.storage.static_files_dir = Some(PathBuf::from("./data/static"));
    let error = validate_config(&ephemeral, 0x3554_334E)
        .expect_err("ephemeral canonical storage cannot own a durable mirror");
    assert!(error.to_string().contains("durable canonical MDBX backend"));

    let mut read_only = NodeConfig::default();
    read_only.storage.backend = Some("mdbx".to_string());
    read_only.storage.data_dir = Some(PathBuf::from("./data/chain"));
    read_only.storage.read_only = true;
    read_only.storage.static_files_dir = Some(PathBuf::from("./data/static"));
    let error = validate_config(&read_only, 0x3554_334E)
        .expect_err("read-only canonical storage cannot publish archive frames");
    assert!(error.to_string().contains("read_only"));
}

#[test]
fn validate_config_for_ledger_mode_allows_static_files_with_storage_override() {
    let mut config = NodeConfig::default();
    config.storage.backend = Some("memory".to_string());
    config.storage.static_files_dir = Some(PathBuf::from("./data/static"));

    validate_config_for_ledger_mode(
        &config,
        0x3554_334E,
        LedgerMode::Local,
        Some(Path::new("./data/chain")),
    )
    .expect("CLI storage override should satisfy durable local storage validation");
}

#[test]
fn validate_config_for_ledger_mode_rejects_truly_in_memory_static_files_in_local_mode() {
    let mut config = NodeConfig::default();
    config.storage.backend = Some("memory".to_string());
    config.storage.static_files_dir = Some(PathBuf::from("./data/static"));

    let error = validate_config_for_ledger_mode(&config, 0x3554_334E, LedgerMode::Local, None)
        .expect_err("local mode without durable storage must still reject static files");
    assert!(error.to_string().contains("durable canonical MDBX backend"));
}

#[test]
fn validate_config_rejects_specialization_shadow_without_local_execution() {
    let config: NodeConfig = toml::from_str(
        r#"
[execution.specialization_shadow]
enabled = true
candidates = ["flamingo_factory_pair_key_v1"]
"#,
    )
    .expect("parse shadow config");

    let error = validate_config_for_ledger_mode(
        &config,
        0x334F_454E,
        LedgerMode::RemoteRpc {
            endpoint: "https://rpc.example.invalid",
        },
        None,
    )
    .expect_err("remote-ledger mode does not execute a local persistence pipeline");
    assert!(
        error
            .to_string()
            .contains("requires local ledger execution")
    );
}

#[test]
fn validate_config_rejects_optimistic_signature_verification_without_local_execution() {
    let config: NodeConfig = toml::from_str(
        r#"
[execution.optimistic_signature_verification]
enabled = true
workers = 2
queue_capacity = 8
"#,
    )
    .expect("parse optimistic signature config");

    let error = validate_config_for_ledger_mode(
        &config,
        0x334F_454E,
        LedgerMode::RemoteRpc {
            endpoint: "https://rpc.example.invalid",
        },
        None,
    )
    .expect_err("remote-ledger mode has no local header verification snapshot");
    assert!(
        error
            .to_string()
            .contains("optimistic_signature_verification")
    );
}

#[test]
fn validate_config_rejects_zero_static_file_resource_limits() {
    let mut config = NodeConfig::default();
    config.storage.backend = Some("mdbx".to_string());
    config.storage.data_dir = Some(PathBuf::from("./data/chain"));
    config.storage.static_files_dir = Some(PathBuf::from("./data/static"));
    config.storage.static_files_cache_capacity = Some(0);
    let error = validate_config(&config, 0x3554_334E).expect_err("zero cache must fail");
    assert!(error.to_string().contains("cache_capacity"));

    config.storage.static_files_cache_capacity = Some(1);
    config.storage.static_files_max_segment_mb = Some(0);
    let error = validate_config(&config, 0x3554_334E).expect_err("zero segment cap must fail");
    assert!(error.to_string().contains("max_segment_bytes"));

    config.storage.static_files_max_segment_mb = Some(1);
    config.storage.static_files_recovery_batch_blocks = Some(0);
    let error = validate_config(&config, 0x3554_334E).expect_err("zero batch must fail");
    assert!(error.to_string().contains("recovery_batch_blocks"));
}

#[test]
fn validate_config_rejects_observability_without_endpoints() {
    let config: NodeConfig = toml::from_str(
        r#"
[observability]
enabled = true
"#,
    )
    .expect("parse config");

    let err = validate_config(&config, 0x3554_334E)
        .expect_err("enabled observability without endpoints should fail");
    assert!(
        err.to_string()
            .contains("requires at least one enabled error or heartbeat endpoint"),
        "{err}"
    );
}

#[test]
fn validate_config_rejects_better_stack_logs_without_token() {
    let config: NodeConfig = toml::from_str(
        r#"
[observability]
enabled = true

[[observability.error_endpoints]]
kind = "better_stack_logs"
url = "https://in.logs.betterstack.com"
"#,
    )
    .expect("parse config");

    let err = validate_config(&config, 0x3554_334E)
        .expect_err("Better Stack logs endpoint requires a token");
    assert!(
        err.to_string().contains("requires token or token_env"),
        "{err}"
    );
}

#[test]
fn validate_config_rejects_google_error_reporting_project_without_token() {
    let config: NodeConfig = toml::from_str(
        r#"
[observability]
enabled = true

[[observability.error_endpoints]]
kind = "google_error_reporting"
project_id = "neo-production"
"#,
    )
    .expect("parse config");

    let err = validate_config(&config, 0x3554_334E)
        .expect_err("Google Error Reporting project endpoint requires a token");
    assert!(
        err.to_string()
            .contains("requires token or token_env when project_id is used without url"),
        "{err}"
    );
}

#[test]
fn validate_config_allows_google_error_reporting_custom_url_without_token() {
    let config: NodeConfig = toml::from_str(
        r#"
[observability]
enabled = true

[[observability.error_endpoints]]
kind = "google_error_reporting"
url = "https://clouderrorreporting.googleapis.com/v1beta1/projects/neo-production/events:report?key=example"
"#,
    )
    .expect("parse config");

    validate_config(&config, 0x3554_334E)
        .expect("custom Google Error Reporting URL can carry its own credentials");
}

#[test]
fn validate_config_allows_sentry_error_endpoint_with_custom_auth_header() {
    let config: NodeConfig = toml::from_str(
        r#"
[observability]
enabled = true

[[observability.error_endpoints]]
kind = "sentry"
url = "https://sentry.example.com/api/42/store/"

[observability.error_endpoints.headers]
X-Sentry-Auth = "Sentry sentry_key=public, sentry_version=7"
"#,
    )
    .expect("parse config");

    validate_config(&config, 0x3554_334E)
        .expect("Sentry endpoint can authenticate through provider-specific headers");
}

#[test]
fn validate_config_rejects_duplicate_static_and_env_observability_headers() {
    let config: NodeConfig = toml::from_str(
        r#"
[observability]
enabled = true

[[observability.error_endpoints]]
kind = "sentry"
url = "https://sentry.example.com/api/42/store/"

[observability.error_endpoints.headers]
X-Sentry-Auth = "inline-secret"

[observability.error_endpoints.headers_env]
x-sentry-auth = "SENTRY_AUTH_HEADER"
"#,
    )
    .expect("parse config");

    let err =
        validate_config(&config, 0x3554_334E).expect_err("duplicate static/env header should fail");
    assert!(
        err.to_string()
            .contains("must not configure the same header in both headers and headers_env"),
        "{err}"
    );
}

#[test]
fn validate_config_rejects_panic_capture_without_error_endpoint() {
    let config: NodeConfig = toml::from_str(
        r#"
[observability]
enabled = true

[[observability.heartbeat_endpoints]]
url = "https://uptime.betterstack.com/api/v1/heartbeat/example"
"#,
    )
    .expect("parse config");

    let err = validate_config(&config, 0x3554_334E)
        .expect_err("panic capture without error endpoint should fail");
    assert!(
        err.to_string()
            .contains("capture_panics requires at least one enabled error endpoint"),
        "{err}"
    );
}

#[test]
fn validate_config_allows_heartbeat_only_when_panic_capture_is_disabled() {
    let config: NodeConfig = toml::from_str(
        r#"
[observability]
enabled = true
capture_panics = false

[[observability.heartbeat_endpoints]]
url = "https://uptime.betterstack.com/api/v1/heartbeat/example"
"#,
    )
    .expect("parse config");

    validate_config(&config, 0x3554_334E).expect("heartbeat-only config is valid");
}

#[test]
fn validate_config_rejects_invalid_logging_format() {
    let config: NodeConfig = toml::from_str(
        r#"
[logging]
format = "xml"
"#,
    )
    .expect("parse config");

    let err =
        validate_config(&config, 0x3554_334E).expect_err("invalid logging format should fail");
    assert!(
        err.to_string().contains("unsupported [logging].format"),
        "{err}"
    );
}

#[test]
fn validate_config_rejects_log_rotation_without_file_path() {
    let config: NodeConfig = toml::from_str(
        r#"
[logging]
max_file_size = "10MB"
"#,
    )
    .expect("parse config");

    let err = validate_config(&config, 0x3554_334E)
        .expect_err("log rotation without a log file should fail");
    assert!(
        err.to_string()
            .contains("[logging].max_file_size requires [logging].file_path"),
        "{err}"
    );
}

#[test]
fn validate_config_rejects_invalid_metrics_path() {
    let config: NodeConfig = toml::from_str(
        r#"
[telemetry.metrics]
enabled = true
path = "metrics"
"#,
    )
    .expect("parse config");

    let err = validate_config(&config, 0x3554_334E).expect_err("relative metrics path should fail");
    assert!(
        err.to_string()
            .contains("[telemetry.metrics].path must start with '/'"),
        "{err}"
    );
}

#[test]
fn validate_config_rejects_reserved_metrics_paths() {
    for path in ["/healthz", "/readyz"] {
        let config: NodeConfig = toml::from_str(&format!(
            r#"
[telemetry.metrics]
enabled = true
path = "{path}"
"#
        ))
        .expect("parse config");

        let err =
            validate_config(&config, 0x3554_334E).expect_err("reserved metrics path should fail");
        assert!(
            err.to_string()
                .contains("reserved for the built-in health endpoint"),
            "{err}"
        );
    }
}

#[test]
fn validate_config_rejects_invalid_metrics_bind_address() {
    let config: NodeConfig = toml::from_str(
        r#"
[telemetry.metrics]
enabled = true
bind_address = "not-an-ip"
"#,
    )
    .expect("parse config");

    let err = validate_config(&config, 0x3554_334E)
        .expect_err("invalid metrics bind address should fail");
    assert!(
        err.to_string()
            .contains("invalid [telemetry.metrics].bind_address"),
        "{err}"
    );
}

#[test]
fn validate_config_rejects_non_http_observability_url() {
    let config: NodeConfig = toml::from_str(
        r#"
[observability]
enabled = true
capture_panics = false

[[observability.heartbeat_endpoints]]
url = "file:///tmp/neo-heartbeat"
"#,
    )
    .expect("parse config");

    let err =
        validate_config(&config, 0x3554_334E).expect_err("non-http observability URL should fail");
    assert!(err.to_string().contains("unsupported URL scheme"), "{err}");
}

#[test]
fn validate_config_rejects_observability_url_surrounding_whitespace() {
    let config: NodeConfig = toml::from_str(
        r#"
[observability]
enabled = true
capture_panics = false

[[observability.heartbeat_endpoints]]
url = " https://uptime.example.com/neo-node "
"#,
    )
    .expect("parse config");

    let err = validate_config(&config, 0x3554_334E)
        .expect_err("observability URL with surrounding whitespace should fail");
    assert!(
        err.to_string().contains(
            "[[observability.heartbeat_endpoints]].url must not contain surrounding whitespace"
        ),
        "{err}"
    );
}

#[test]
fn validate_config_rejects_blank_observability_tokens() {
    let config: NodeConfig = toml::from_str(
        r#"
[observability]
enabled = true

[[observability.error_endpoints]]
kind = "custom_json"
url = "https://errors.example.com/neo-node"
token = "   "
"#,
    )
    .expect("parse config");

    let err =
        validate_config(&config, 0x3554_334E).expect_err("blank observability token should fail");
    assert!(err.to_string().contains("token must not be empty"), "{err}");
}

#[test]
fn validate_config_rejects_observability_token_surrounding_whitespace() {
    let config: NodeConfig = toml::from_str(
        r#"
[observability]
enabled = true

[[observability.error_endpoints]]
kind = "custom_json"
url = "https://errors.example.com/neo-node"
token = " secret "
"#,
    )
    .expect("parse config");

    let err =
        validate_config(&config, 0x3554_334E).expect_err("spaced observability token should fail");
    assert!(
        err.to_string()
            .contains("token must not contain surrounding whitespace"),
        "{err}"
    );
}

#[test]
fn validate_config_rejects_observability_token_env_surrounding_whitespace() {
    let config: NodeConfig = toml::from_str(
        r#"
[observability]
enabled = true

[[observability.error_endpoints]]
kind = "custom_json"
url = "https://errors.example.com/neo-node"
token_env = " NEO_OBSERVABILITY_TOKEN "
"#,
    )
    .expect("parse config");

    let err = validate_config(&config, 0x3554_334E)
        .expect_err("token_env with surrounding whitespace should fail");
    assert!(
        err.to_string()
            .contains("token_env must not contain surrounding whitespace"),
        "{err}"
    );
}

#[test]
fn validate_config_rejects_invalid_observability_headers() {
    let config: NodeConfig = toml::from_str(
        r#"
[observability]
enabled = true
capture_panics = false

[[observability.heartbeat_endpoints]]
url = "https://uptime.example.com/neo-node"

[observability.heartbeat_endpoints.headers]
"bad header" = "value"
"#,
    )
    .expect("parse config");

    let err = validate_config(&config, 0x3554_334E).expect_err("invalid header name should fail");
    assert!(
        err.to_string().contains("invalid HTTP header name"),
        "{err}"
    );
}

#[test]
fn validate_config_rejects_observability_authorization_header_with_token() {
    let config: NodeConfig = toml::from_str(
        r#"
[observability]
enabled = true
capture_panics = false

[[observability.heartbeat_endpoints]]
url = "https://uptime.example.com/neo-node"
token_env = "NEO_HEARTBEAT_TOKEN"

[observability.heartbeat_endpoints.headers]
Authorization = "Bearer other"
"#,
    )
    .expect("parse config");

    let err = validate_config(&config, 0x3554_334E)
        .expect_err("authorization header and token should conflict");
    assert!(
        err.to_string()
            .contains("must not include Authorization when token or token_env is configured"),
        "{err}"
    );
}

#[test]
fn validate_config_rejects_empty_indexer_store_path() {
    let mut config = NodeConfig::default();
    config.indexer.enabled = true;
    config.indexer.store_path = Some(PathBuf::new());

    let err =
        validate_config(&config, 0x3554_334E).expect_err("empty indexer store path should fail");
    assert!(
        err.to_string()
            .contains("[indexer].store_path must not be empty"),
        "{err}"
    );
}

#[test]
fn validate_config_rejects_empty_application_logs_path() {
    let config: NodeConfig = toml::from_str(
        r#"
[application_logs]
enabled = true
path = ""
"#,
    )
    .expect("parse config");

    let err =
        validate_config(&config, 0x004F_454E).expect_err("empty ApplicationLogs path should fail");
    assert!(
        err.to_string()
            .contains("[application_logs].path must not be empty"),
        "{err}"
    );
}

#[test]
fn validate_config_rejects_empty_tokens_tracker_path() {
    let config: NodeConfig = toml::from_str(
        r#"
[tokens_tracker]
enabled = true
DBPath = ""
"#,
    )
    .expect("parse config");

    let err =
        validate_config(&config, 0x004F_454E).expect_err("empty TokensTracker path should fail");
    assert!(
        err.to_string()
            .contains("[tokens_tracker].db_path must not be empty"),
        "{err}"
    );
}

#[test]
fn validate_config_rejects_empty_state_service_path() {
    let mut config = NodeConfig::default();
    config.state_service.enabled = true;
    config.state_service.path = Some(PathBuf::new());

    let err =
        validate_config(&config, 0x004F_454E).expect_err("empty StateService path should fail");
    assert!(
        err.to_string()
            .contains("[state_service].path must not be empty"),
        "{err}"
    );
}

#[test]
fn validate_config_rejects_deferred_full_state_without_full_state() {
    let mut config = NodeConfig::default();
    config.state_service.enabled = true;
    config.state_service.defer_full_state_finalization = true;

    let err = validate_config(&config, 0x004F_454E)
        .expect_err("deferred full-state finalization requires full-state retention");
    assert!(
        err.to_string()
            .contains("defer_full_state_finalization requires")
    );
}

#[test]
fn validate_config_skips_local_replay_service_paths_for_remote_ledger_mode() {
    let config: NodeConfig = toml::from_str(
        r#"
[state_service]
enabled = true
path = ""

[indexer]
enabled = true
store_path = ""

[application_logs]
enabled = true
path = ""

[tokens_tracker]
enabled = true
DBPath = ""
"#,
    )
    .expect("parse remote-ledger service config");

    validate_config_for_ledger_mode(
        &config,
        0x004F_454E,
        LedgerMode::RemoteRpc {
            endpoint: "https://rpc.example.invalid",
        },
        None,
    )
    .expect("remote-ledger mode must ignore disabled local replay service paths");
}

#[test]
fn validate_config_rejects_service_store_paths_that_are_files() {
    let temp = tempfile::tempdir().expect("temp dir");

    let state_path = temp.path().join("StateService_004F454E");
    std::fs::write(&state_path, b"not a state store").expect("create state service file");
    let mut state_config = NodeConfig::default();
    state_config.state_service.enabled = true;
    state_config.state_service.path = Some(temp.path().join("StateService_{0}"));
    let err = validate_config(&state_config, 0x004F_454E)
        .expect_err("StateService path pointing to a file should fail");
    assert!(
        err.to_string()
            .contains("[state_service].path must be a service-store directory"),
        "{err}"
    );

    let logs_path = temp.path().join("ApplicationLogs_004F454E");
    std::fs::write(&logs_path, b"not application logs").expect("create ApplicationLogs file");
    let logs_config: NodeConfig = toml::from_str(&format!(
        r#"
[application_logs]
enabled = true
path = "{}"
"#,
        temp.path().join("ApplicationLogs_{0}").display()
    ))
    .expect("parse ApplicationLogs config");
    let err = validate_config(&logs_config, 0x004F_454E)
        .expect_err("ApplicationLogs path pointing to a file should fail");
    assert!(
        err.to_string()
            .contains("[application_logs].path must be a service-store directory"),
        "{err}"
    );

    let tokens_path = temp.path().join("TokenBalanceData");
    std::fs::write(&tokens_path, b"not token tracker data").expect("create TokensTracker file");
    let tokens_config: NodeConfig = toml::from_str(&format!(
        r#"
[tokens_tracker]
enabled = true
DBPath = "{}"
"#,
        tokens_path.display()
    ))
    .expect("parse TokensTracker config");
    let err = validate_config(&tokens_config, 0x004F_454E)
        .expect_err("TokensTracker path pointing to a file should fail");
    assert!(
        err.to_string()
            .contains("[tokens_tracker].db_path must be a service-store directory"),
        "{err}"
    );
}

#[test]
fn validate_config_rejects_indexer_store_path_file_after_network_expansion() {
    let temp = tempfile::tempdir().expect("temp dir");
    let expanded = temp.path().join("Indexer_004F454E");
    std::fs::write(&expanded, b"not a store directory").expect("create file at store path");

    let mut config = NodeConfig::default();
    config.indexer.enabled = true;
    config.indexer.store_path = Some(temp.path().join("Indexer_{0}"));

    let err =
        validate_config(&config, 0x004F_454E).expect_err("file indexer store path should fail");
    assert!(
        err.to_string()
            .contains("must be a service-store directory"),
        "{err}"
    );
}

#[test]
fn validate_storage_requires_mdbx_path() {
    let config: NodeConfig = toml::from_str(
        r#"
[storage]
backend = "mdbx"
"#,
    )
    .expect("parse config");

    let err = validate_storage(&config, None, 0x334F_454E).expect_err("missing path fails");
    assert!(err.to_string().contains("requires a data directory"));
}

#[test]
fn validate_storage_rejects_local_replay_poison_marker() {
    let temp = tempfile::tempdir().expect("temp dir");
    let chain_path = temp.path().join("chain");
    std::fs::create_dir_all(&chain_path).expect("create chain directory");
    let marker = chain_path.join(".neo-local-replay-poisoned");
    std::fs::write(&marker, b"reason=injected commit failure\n").expect("write marker");

    let mut config = NodeConfig::default();
    config.storage.backend = Some("mdbx".to_string());
    config.storage.data_dir = Some(chain_path);

    let error = validate_storage(&config, None, ProtocolSettings::default().network)
        .expect_err("storage preflight must reject poisoned local replay");
    assert!(error.to_string().contains("local replay is poisoned"));
    assert!(error.to_string().contains(&marker.display().to_string()));
}

#[test]
fn validate_storage_rejects_state_service_root_for_uninitialized_chain() {
    let temp = tempfile::tempdir().expect("temp dir");
    let chain_path = temp.path().join("chain");
    let settings = ProtocolSettings::default();
    seed_coordinated_mdbx_state_height(&chain_path, 0u32.to_le_bytes().to_vec());

    let mut config = NodeConfig::default();
    config.storage.backend = Some("mdbx".to_string());
    config.storage.data_dir = Some(chain_path);
    config.state_service.enabled = true;

    let error = validate_storage(&config, None, settings.network)
        .expect_err("StateService must not be ahead of an uninitialized chain");
    assert!(
        error.to_string().contains("chain store is uninitialized"),
        "{error}"
    );
}

fn seed_coordinated_mdbx_state_height(path: &Path, bytes: Vec<u8>) {
    let mut open_config = NodeConfig::default();
    open_config.storage.backend = Some("mdbx".to_string());
    let canonical = open_store(&open_config, Some(path)).expect("open canonical MDBX store");
    let state_store = canonical
        .open_coordinated_namespace(neo_state_service::MDBX_STATE_SERVICE_NAMESPACE)
        .expect("open coordinated StateService namespace");
    let mut snapshot = state_store.snapshot();
    let writer = Arc::get_mut(&mut snapshot).expect("exclusive StateService snapshot");
    writer
        .put(vec![0x02], bytes)
        .expect("write current local root index");
    writer
        .try_commit()
        .expect("commit coordinated StateService height");
}

#[test]
fn validate_storage_reopens_coordinated_mdbx_state_service_namespace() {
    let temp = tempfile::tempdir().expect("temp dir");
    let chain_path = temp.path().join("chain");
    let chain_spec = neo_config::NeoChainSpec::mainnet().expect("valid MainNet chain spec");
    let settings = chain_spec.protocol_settings();
    seed_store_tip("mdbx", &chain_path, chain_spec.as_ref(), 1).expect("seed MDBX chain tip");
    seed_coordinated_mdbx_state_height(&chain_path, 1u32.to_le_bytes().to_vec());

    let mut config = NodeConfig::default();
    config.storage.backend = Some("mdbx".to_string());
    config.storage.data_dir = Some(chain_path);
    config.storage.mdbx_geometry_upper_gb = Some(1);
    config.storage.mdbx_geometry_growth_mb = Some(16);
    config.storage.mdbx_max_readers = Some(128);
    config.state_service.enabled = true;

    validate_storage(&config, None, settings.network)
        .expect("matching coordinated MDBX StateService height validates after reopen");
}

#[test]
fn validate_storage_rejects_coordinated_mdbx_state_height_mismatch() {
    let temp = tempfile::tempdir().expect("temp dir");
    let chain_path = temp.path().join("chain");
    let chain_spec = neo_config::NeoChainSpec::mainnet().expect("valid MainNet chain spec");
    let settings = chain_spec.protocol_settings();
    seed_store_tip("mdbx", &chain_path, chain_spec.as_ref(), 1).expect("seed MDBX chain tip");
    seed_coordinated_mdbx_state_height(&chain_path, 0u32.to_le_bytes().to_vec());

    let mut config = NodeConfig::default();
    config.storage.backend = Some("mdbx".to_string());
    config.storage.data_dir = Some(chain_path);
    config.state_service.enabled = true;

    let error = validate_storage(&config, None, settings.network)
        .expect_err("mismatched coordinated StateService height must fail");
    assert!(
        error.to_string().contains("does not match chain height 1"),
        "{error}"
    );
    assert!(error.to_string().contains("neo_state_service"), "{error}");
}

#[test]
fn validate_storage_rejects_malformed_coordinated_mdbx_state_height() {
    let temp = tempfile::tempdir().expect("temp dir");
    let chain_path = temp.path().join("chain");
    let chain_spec = neo_config::NeoChainSpec::mainnet().expect("valid MainNet chain spec");
    let settings = chain_spec.protocol_settings();
    seed_store_tip("mdbx", &chain_path, chain_spec.as_ref(), 1).expect("seed MDBX chain tip");
    seed_coordinated_mdbx_state_height(&chain_path, vec![0x01, 0x02, 0x03]);

    let mut config = NodeConfig::default();
    config.storage.backend = Some("mdbx".to_string());
    config.storage.data_dir = Some(chain_path);
    config.state_service.enabled = true;

    let error = validate_storage(&config, None, settings.network)
        .expect_err("malformed coordinated StateService height must fail");
    assert!(error.to_string().contains("expected 4 bytes"), "{error}");
    assert!(error.to_string().contains("neo_state_service"), "{error}");
}

#[test]
fn validate_storage_rejects_missing_coordinated_state_root_for_populated_chain() {
    let temp = tempfile::tempdir().expect("temp dir");
    let chain_path = temp.path().join("chain");
    let chain_spec = neo_config::NeoChainSpec::mainnet().expect("valid MainNet chain spec");
    let settings = chain_spec.protocol_settings();
    seed_mdbx_tip(&chain_path, chain_spec.as_ref(), 1).expect("seed chain tip");

    let mut config = NodeConfig::default();
    config.storage.backend = Some("mdbx".to_string());
    config.storage.data_dir = Some(chain_path);
    config.state_service.enabled = true;

    let err = validate_storage(&config, None, settings.network)
        .expect_err("populated chain requires a coordinated StateService root");
    assert!(err.to_string().contains("no current local root"), "{err}");
}

/// Default P2P ports follow the network magic.
#[test]
fn default_p2p_port_matches_network() {
    assert_eq!(default_p2p_port(0x3554_334E), 20333);
    assert_eq!(default_p2p_port(0x334F_454E), 10333);
    assert_eq!(default_p2p_port(0xDEAD_BEEF), 0);
}

/// Primary chain storage uses canonical `data_dir`; legacy `[storage].path`
/// must not configure the persistent store.
#[test]
fn storage_section_requires_data_dir_for_primary_path() {
    let toml = "[storage]\nbackend = \"mdbx\"\ndata_dir = \"./data/mainnet\"\n";
    let config: NodeConfig = toml::from_str(toml).expect("parses");
    assert_eq!(config.storage.backend.as_deref(), Some("mdbx"));
    assert_eq!(
        config.storage.data_directory(),
        Some(std::path::PathBuf::from("./data/mainnet"))
    );

    let legacy: NodeConfig =
        toml::from_str("[storage]\nbackend = \"mdbx\"\npath = \"./data/mainnet\"\n")
            .expect("parses unknown path key");
    assert!(legacy.storage.data_directory().is_none());
}
