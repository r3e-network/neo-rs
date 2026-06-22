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
fn runtime_rejects_zero_heartbeat_interval_before_spawning_tasks() {
    let config = ObservabilitySection {
        enabled: true,
        capture_panics: false,
        heartbeat_interval_seconds: 0,
        heartbeat_endpoints: vec![ObservabilityHeartbeatEndpoint {
            url: Some("https://uptime.example.com/heartbeat".to_string()),
            ..ObservabilityHeartbeatEndpoint::default()
        }],
        ..ObservabilitySection::default()
    };

    let error = expect_runtime_config_error(&config);

    assert!(
        error.contains("[observability].heartbeat_interval_seconds must be greater than zero"),
        "unexpected error: {error}"
    );
}

#[test]
fn runtime_rejects_enabled_without_observability_destinations() {
    let config = ObservabilitySection {
        enabled: true,
        capture_panics: false,
        ..ObservabilitySection::default()
    };

    let error = expect_runtime_config_error(&config);

    assert!(
        error.contains(
            "[observability].enabled requires at least one enabled error or heartbeat endpoint"
        ),
        "unexpected error: {error}"
    );
}

#[test]
fn runtime_rejects_panic_capture_without_error_endpoint() {
    let config = ObservabilitySection {
        enabled: true,
        capture_panics: true,
        heartbeat_endpoints: vec![ObservabilityHeartbeatEndpoint {
            url: Some("https://uptime.example.com/heartbeat".to_string()),
            ..ObservabilityHeartbeatEndpoint::default()
        }],
        error_endpoints: vec![ObservabilityErrorEndpoint {
            enabled: false,
            ..ObservabilityErrorEndpoint::default()
        }],
        ..ObservabilitySection::default()
    };

    let error = expect_runtime_config_error(&config);

    assert!(
        error.contains(
            "[observability].capture_panics requires at least one enabled error endpoint"
        ),
        "unexpected error: {error}"
    );
}
