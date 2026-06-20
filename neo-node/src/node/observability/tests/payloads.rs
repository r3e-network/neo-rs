use serde_json::json;

use super::super::payloads::{
    ErrorReport, ObservabilityMetadata, ReportLocation, build_better_stack_error_payload,
    build_generic_error_payload, build_google_error_payload, build_heartbeat_payload,
    build_sentry_error_payload,
};

#[test]
fn generic_payload_includes_network_and_location() {
    let metadata = ObservabilityMetadata {
        service_name: "neo-node-mainnet".to_string(),
        environment: Some("production".to_string()),
        node_id: Some("validator-1".to_string()),
        network: 0x334F_454E,
        version: "test",
    };
    let report = ErrorReport {
        event_type: "panic".to_string(),
        message: "boom".to_string(),
        timestamp: "2026-06-19T00:00:00Z".to_string(),
        location: Some(ReportLocation {
            file_path: "src/node.rs".to_string(),
            line_number: 42,
            column_number: 7,
        }),
    };

    let payload = build_generic_error_payload(&metadata, &report);
    assert_eq!(payload["service"]["name"], "neo-node-mainnet");
    assert_eq!(payload["service"]["network"], "0x334F454E");
    assert_eq!(payload["event"]["location"]["line_number"], 42);
}

#[test]
fn google_payload_uses_error_reporting_shape() {
    let metadata = ObservabilityMetadata {
        service_name: "neo-node".to_string(),
        environment: Some("testnet".to_string()),
        node_id: Some("node-1".to_string()),
        network: 0x3554_334E,
        version: "test",
    };
    let report = ErrorReport {
        event_type: "startup_error".to_string(),
        message: "failed".to_string(),
        timestamp: "2026-06-19T00:00:00Z".to_string(),
        location: None,
    };

    let payload = build_google_error_payload(&metadata, &report);
    assert_eq!(payload["serviceContext"]["service"], "neo-node");
    assert_eq!(payload["eventTime"], "2026-06-19T00:00:00Z");
    assert_eq!(payload["message"], "startup_error: failed");
    assert_eq!(payload["context"]["user"], "node-1");
    assert_eq!(payload["context"]["reportLocation"]["filePath"], "neo-node");
    assert_eq!(payload["context"]["reportLocation"]["lineNumber"], 0);
    assert_eq!(
        payload["context"]["reportLocation"]["functionName"],
        "startup_error"
    );
}

#[test]
fn google_payload_preserves_panic_source_location() {
    let metadata = ObservabilityMetadata {
        service_name: "neo-node".to_string(),
        environment: None,
        node_id: None,
        network: 0x3554_334E,
        version: "test",
    };
    let report = ErrorReport {
        event_type: "panic".to_string(),
        message: "boom".to_string(),
        timestamp: "2026-06-19T00:00:00Z".to_string(),
        location: Some(ReportLocation {
            file_path: "src/node.rs".to_string(),
            line_number: 42,
            column_number: 7,
        }),
    };

    let payload = build_google_error_payload(&metadata, &report);

    assert_eq!(
        payload["context"]["reportLocation"]["filePath"],
        "src/node.rs"
    );
    assert_eq!(payload["context"]["reportLocation"]["lineNumber"], 42);
    assert_eq!(
        payload["context"]["reportLocation"]["functionName"],
        "panic"
    );
    assert!(payload["context"].get("user").is_none());
}

#[test]
fn better_stack_payload_uses_log_event_shape() {
    let metadata = ObservabilityMetadata {
        service_name: "neo-node-mainnet".to_string(),
        environment: Some("production".to_string()),
        node_id: Some("validator-1".to_string()),
        network: 0x334F_454E,
        version: "test",
    };
    let report = ErrorReport {
        event_type: "startup_error".to_string(),
        message: "database open failed".to_string(),
        timestamp: "2026-06-19T00:00:00Z".to_string(),
        location: Some(ReportLocation {
            file_path: "src/node.rs".to_string(),
            line_number: 42,
            column_number: 7,
        }),
    };

    let payload = build_better_stack_error_payload(&metadata, &report);

    assert_eq!(payload["message"], "database open failed");
    assert_eq!(payload["dt"], "2026-06-19T00:00:00Z");
    assert_eq!(payload["level"], "error");
    assert_eq!(payload["event_type"], "startup_error");
    assert_eq!(payload["service"], "neo-node-mainnet");
    assert_eq!(payload["environment"], "production");
    assert_eq!(payload["node_id"], "validator-1");
    assert_eq!(payload["network"], "0x334F454E");
    assert_eq!(payload["version"], "test");
    assert_eq!(payload["location"]["file_path"], "src/node.rs");
    assert_eq!(payload["location"]["line_number"], 42);
}

#[test]
fn sentry_payload_uses_event_shape_and_tags() {
    let metadata = ObservabilityMetadata {
        service_name: "neo-node-mainnet".to_string(),
        environment: Some("production".to_string()),
        node_id: Some("validator-1".to_string()),
        network: 0x334F_454E,
        version: "test",
    };
    let report = ErrorReport {
        event_type: "panic".to_string(),
        message: "boom".to_string(),
        timestamp: "2026-06-19T00:00:00Z".to_string(),
        location: Some(ReportLocation {
            file_path: "src/node.rs".to_string(),
            line_number: 42,
            column_number: 7,
        }),
    };

    let payload = build_sentry_error_payload(&metadata, &report);

    assert_eq!(payload["timestamp"], "2026-06-19T00:00:00Z");
    assert_eq!(payload["platform"], "rust");
    assert_eq!(payload["level"], "fatal");
    assert_eq!(payload["logger"], "neo-node");
    assert_eq!(payload["transaction"], "panic");
    assert_eq!(payload["message"], "boom");
    assert_eq!(payload["release"], "neo-node@test");
    assert_eq!(payload["environment"], "production");
    assert_eq!(payload["server_name"], "validator-1");
    assert_eq!(payload["tags"]["service"], "neo-node-mainnet");
    assert_eq!(payload["tags"]["network"], "0x334F454E");
    assert_eq!(payload["tags"]["event_type"], "panic");
    assert_eq!(payload["extra"]["node_id"], "validator-1");
    assert_eq!(
        payload["exception"]["values"][0]["stacktrace"]["frames"][0]["filename"],
        "src/node.rs"
    );
    assert_eq!(
        payload["exception"]["values"][0]["stacktrace"]["frames"][0]["lineno"],
        42
    );
    assert_eq!(
        payload["exception"]["values"][0]["stacktrace"]["frames"][0]["colno"],
        7
    );
}

#[test]
fn heartbeat_payload_includes_node_health_when_supplied() {
    let metadata = ObservabilityMetadata {
        service_name: "neo-node".to_string(),
        environment: Some("testnet".to_string()),
        node_id: Some("node-1".to_string()),
        network: 0x3554_334E,
        version: "test",
    };
    let node_health = json!({
        "ready": true,
        "ledger_height": 42,
        "services": {
            "indexer": {
                "enabled": true,
                "ready": true,
                "blocks_behind": 0,
            }
        }
    });

    let payload = build_heartbeat_payload(&metadata, Some(node_health));

    assert_eq!(payload["event"]["type"], "heartbeat");
    assert_eq!(payload["service"]["network"], "0x3554334E");
    assert_eq!(payload["node"]["ready"], true);
    assert_eq!(payload["node"]["ledger_height"], 42);
    assert_eq!(payload["node"]["services"]["indexer"]["blocks_behind"], 0);
}

#[test]
fn heartbeat_failure_report_is_error_payload_compatible() {
    let metadata = ObservabilityMetadata {
        service_name: "neo-node".to_string(),
        environment: Some("production".to_string()),
        node_id: Some("validator-7".to_string()),
        network: 0x334F_454E,
        version: "test",
    };
    let report = ErrorReport::heartbeat_failure(
        "better-stack-heartbeat",
        &anyhow::anyhow!("heartbeat endpoint returned HTTP 500"),
    );

    let generic = build_generic_error_payload(&metadata, &report);
    assert_eq!(generic["event"]["type"], "heartbeat_failure");
    assert_eq!(
        generic["event"]["message"],
        "heartbeat better-stack-heartbeat failed: heartbeat endpoint returned HTTP 500"
    );
    assert!(generic["event"]["location"].is_null());

    let better_stack = build_better_stack_error_payload(&metadata, &report);
    assert_eq!(better_stack["level"], "error");
    assert_eq!(better_stack["event_type"], "heartbeat_failure");

    let google = build_google_error_payload(&metadata, &report);
    assert_eq!(
        google["message"],
        "heartbeat_failure: heartbeat better-stack-heartbeat failed: heartbeat endpoint returned HTTP 500"
    );
    assert_eq!(
        google["context"]["reportLocation"]["functionName"],
        "heartbeat_failure"
    );
}
