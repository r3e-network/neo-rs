//! Tests that destination URLs never leak secrets into logs or forwarded reports.

use super::super::super::config::ObservabilityHeartbeatEndpoint;
use super::super::endpoints::{heartbeat_endpoint_name, redact_url};

#[test]
fn redact_url_strips_secret_path_query_and_credentials() {
    // Better Stack-style heartbeat secret lives in the path; it must be stripped.
    assert_eq!(
        redact_url("https://uptime.betterstack.com/api/v1/heartbeat/super-secret-id"),
        "https://uptime.betterstack.com"
    );
    // Query strings (which may carry API keys) are removed.
    assert_eq!(
        redact_url("https://errors.example.com/store?sentry_key=abc123"),
        "https://errors.example.com"
    );
    // Embedded basic-auth credentials are removed.
    assert_eq!(
        redact_url("https://user:password@logs.example.com/ingest"),
        "https://logs.example.com"
    );
    // Non-URL inputs never echo their contents.
    assert_eq!(redact_url("not-a-url"), "<redacted-url>");
}

#[test]
fn heartbeat_endpoint_name_never_leaks_the_secret_url() {
    // With no operator-provided name, the label must be the redacted host, not the
    // raw URL (which embeds the secret heartbeat id).
    let endpoint = ObservabilityHeartbeatEndpoint {
        url: Some("https://uptime.betterstack.com/api/v1/heartbeat/super-secret-id".to_string()),
        ..ObservabilityHeartbeatEndpoint::default()
    };
    let name = heartbeat_endpoint_name(&endpoint);
    assert_eq!(name, "https://uptime.betterstack.com");
    assert!(
        !name.contains("super-secret-id"),
        "heartbeat label must not contain the secret id: {name}"
    );

    // An explicit name is used verbatim.
    let named = ObservabilityHeartbeatEndpoint {
        name: Some("primary-uptime".to_string()),
        url: Some("https://uptime.betterstack.com/api/v1/heartbeat/super-secret-id".to_string()),
        ..ObservabilityHeartbeatEndpoint::default()
    };
    assert_eq!(heartbeat_endpoint_name(&named), "primary-uptime");
}
