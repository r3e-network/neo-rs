use super::super::super::config::{
    ObservabilityErrorEndpoint, ObservabilityHeartbeatEndpoint, ObservabilitySection,
};
use super::super::ObservabilityRuntime;

fn expect_runtime_config_error(config: &ObservabilitySection) -> String {
    match ObservabilityRuntime::from_config(config, 0x3554_334E) {
        Ok(_) => panic!("observability runtime config should be rejected"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn runtime_rejects_unknown_error_endpoint_kind_before_reporting() {
    let config = ObservabilitySection {
        enabled: true,
        capture_panics: true,
        error_endpoints: vec![ObservabilityErrorEndpoint {
            kind: Some("sentery".to_string()),
            url: Some("https://errors.example.com/neo-node".to_string()),
            ..ObservabilityErrorEndpoint::default()
        }],
        ..ObservabilitySection::default()
    };

    let error = expect_runtime_config_error(&config);

    assert!(
        error.contains(
            "[[observability.error_endpoints]].kind must be one of custom_json, better_stack_logs, google_error_reporting, or sentry"
        ),
        "unexpected error: {error}"
    );
}

#[test]
fn runtime_rejects_zero_max_send_attempts() {
    let config = ObservabilitySection {
        enabled: true,
        capture_panics: true,
        max_send_attempts: 0,
        error_endpoints: vec![ObservabilityErrorEndpoint {
            kind: Some("custom_json".to_string()),
            url: Some("https://errors.example.com/neo-node".to_string()),
            ..ObservabilityErrorEndpoint::default()
        }],
        ..ObservabilitySection::default()
    };

    let error = expect_runtime_config_error(&config);

    assert!(
        error.contains("[observability].max_send_attempts must be at least 1"),
        "unexpected error: {error}"
    );
}

#[test]
fn runtime_rejects_unknown_heartbeat_method_before_spawning_tasks() {
    let config = ObservabilitySection {
        enabled: true,
        capture_panics: false,
        heartbeat_endpoints: vec![ObservabilityHeartbeatEndpoint {
            url: Some("https://uptime.example.com/heartbeat".to_string()),
            method: Some("PATCH".to_string()),
            ..ObservabilityHeartbeatEndpoint::default()
        }],
        ..ObservabilitySection::default()
    };

    let error = expect_runtime_config_error(&config);

    assert!(
        error.contains(
            "[[observability.heartbeat_endpoints]].method must be one of GET, POST, or PUT"
        ),
        "unexpected error: {error}"
    );
}
