use super::*;

#[test]
fn observability_section_parses_error_and_heartbeat_endpoints() {
    let config: NodeConfig = toml::from_str(
        r#"
[observability]
enabled = true
service_name = "neo-node-testnet"
environment = "testnet"
node_id = "validator-1"
request_timeout_ms = 2500
heartbeat_interval_seconds = 45

[[observability.error_endpoints]]
kind = "google_error_reporting"
project_id = "neo-production"
token_env = "GOOGLE_APPLICATION_TOKEN"

[[observability.error_endpoints]]
name = "custom-json"
kind = "custom_json"
url = "https://errors.example.com/neo-node"

[[observability.heartbeat_endpoints]]
name = "better-stack-heartbeat"
url = "https://uptime.betterstack.com/api/v1/heartbeat/example"
method = "GET"
interval_seconds = 30
"#,
    )
    .expect("parse observability config");

    assert!(config.observability.enabled);
    assert_eq!(
        config.observability.service_name.as_deref(),
        Some("neo-node-testnet")
    );
    assert_eq!(config.observability.error_endpoints.len(), 2);
    assert_eq!(config.observability.heartbeat_endpoints.len(), 1);
    validate_config(&config, 0x3554_334E).expect("valid observability config");
}

#[test]
fn observability_endpoints_parse_header_env_maps() {
    let config: NodeConfig = toml::from_str(
        r#"
[observability]
enabled = true

[[observability.error_endpoints]]
kind = "sentry"
url = "https://sentry.example.com/api/42/store/"

[observability.error_endpoints.headers_env]
X-Sentry-Auth = "SENTRY_AUTH_HEADER"

[[observability.heartbeat_endpoints]]
url = "https://uptime.example.com/heartbeat"

[observability.heartbeat_endpoints.headers_env]
X-Heartbeat-Token = "HEARTBEAT_HEADER_TOKEN"
"#,
    )
    .expect("parse observability config");

    assert_eq!(
        config.observability.error_endpoints[0]
            .headers_env
            .get("X-Sentry-Auth")
            .map(String::as_str),
        Some("SENTRY_AUTH_HEADER")
    );
    assert_eq!(
        config.observability.heartbeat_endpoints[0]
            .headers_env
            .get("X-Heartbeat-Token")
            .map(String::as_str),
        Some("HEARTBEAT_HEADER_TOKEN")
    );
    validate_config(&config, 0x3554_334E).expect("valid header env config");
}

#[test]
fn telemetry_metrics_section_parses_and_derives_endpoint() {
    let config: NodeConfig = toml::from_str(
        r#"
[telemetry.metrics]
enabled = true
port = 19090
bind_address = "127.0.0.1"
path = "/node-metrics"
"#,
    )
    .expect("parse telemetry metrics config");

    assert!(config.telemetry.metrics.enabled);
    assert_eq!(
        config
            .telemetry
            .metrics
            .bind_socket_addr()
            .expect("metrics bind addr")
            .to_string(),
        "127.0.0.1:19090"
    );
    assert_eq!(config.telemetry.metrics.endpoint_path(), "/node-metrics");
    validate_config(&config, 0x3554_334E).expect("valid telemetry metrics config");
}

#[test]
fn logging_section_parses_runtime_tracing_options() {
    let config: NodeConfig = toml::from_str(
        r#"
[logging]
enabled = true
level = "info,neo=debug"
format = "json"
file_path = "./logs/neo-node-test.log"
console_output = true
max_file_size = "100MB"
max_files = 10
"#,
    )
    .expect("parse logging config");

    assert!(config.logging.enabled);
    assert_eq!(config.logging.level.as_deref(), Some("info,neo=debug"));
    assert_eq!(config.logging.format.as_deref(), Some("json"));
    assert_eq!(
        config.logging.file_path.as_deref(),
        Some(std::path::Path::new("./logs/neo-node-test.log"))
    );
    assert_eq!(config.logging.console_output, Some(true));
    assert_eq!(
        config.logging.max_file_size_bytes().expect("log size"),
        Some(100 * 1024 * 1024)
    );
    assert_eq!(config.logging.max_rotated_files(), 10);
    validate_config(&config, 0x3554_334E).expect("valid logging config");
}
