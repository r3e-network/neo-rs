use std::collections::HashMap;

use super::super::super::config::{ObservabilityErrorEndpoint, ObservabilityHeartbeatEndpoint};
use super::super::endpoints::{
    apply_blocking_auth_and_headers, error_endpoint_url, heartbeat_endpoint_url,
};

#[test]
fn error_endpoint_url_with_surrounding_whitespace_is_rejected_before_sending() {
    let endpoint = ObservabilityErrorEndpoint {
        url: Some(" https://errors.example.com/neo-node ".to_string()),
        ..ObservabilityErrorEndpoint::default()
    };

    let error = error_endpoint_url(&endpoint, "custom_json")
        .expect_err("endpoint URL with surrounding whitespace should fail before sending");

    assert!(
        error
            .to_string()
            .contains("endpoint url must not contain surrounding whitespace"),
        "unexpected error: {error}"
    );
}

#[test]
fn heartbeat_endpoint_url_with_surrounding_whitespace_is_rejected_before_sending() {
    let endpoint = ObservabilityHeartbeatEndpoint {
        url: Some(" https://uptime.example.com/neo-node ".to_string()),
        ..ObservabilityHeartbeatEndpoint::default()
    };

    let error = heartbeat_endpoint_url(&endpoint)
        .expect_err("heartbeat URL with surrounding whitespace should fail before sending");

    assert!(
        error
            .to_string()
            .contains("heartbeat URL must not contain surrounding whitespace"),
        "unexpected error: {error}"
    );
}

#[test]
fn env_headers_are_applied_to_outbound_requests() {
    let home = std::env::var("HOME").expect("HOME is set for tests");
    let mut headers_env = HashMap::new();
    headers_env.insert("X-Sentry-Auth".to_string(), "HOME".to_string());
    let request = reqwest::blocking::Client::new().post("https://sentry.example.com/api/42/store/");

    let request =
        apply_blocking_auth_and_headers(request, None, None, &HashMap::new(), &headers_env)
            .expect("apply env headers")
            .build()
            .expect("build request");

    assert_eq!(
        request
            .headers()
            .get("X-Sentry-Auth")
            .expect("header set")
            .to_str()
            .expect("header value"),
        home
    );
}

#[test]
fn blank_header_env_is_rejected_before_sending_outbound_requests() {
    let mut headers_env = HashMap::new();
    headers_env.insert("X-Sentry-Auth".to_string(), "   ".to_string());
    let request = reqwest::blocking::Client::new().post("https://sentry.example.com/api/42/store/");

    let error = apply_blocking_auth_and_headers(request, None, None, &HashMap::new(), &headers_env)
        .expect_err("blank headers_env value should fail before sending");

    assert!(
        error
            .to_string()
            .contains("header env var name for X-Sentry-Auth is empty"),
        "unexpected error: {error}"
    );
}

#[test]
fn header_env_with_surrounding_whitespace_is_rejected_before_sending() {
    let mut headers_env = HashMap::new();
    headers_env.insert("X-Sentry-Auth".to_string(), " HOME ".to_string());
    let request = reqwest::blocking::Client::new().post("https://sentry.example.com/api/42/store/");

    let error = apply_blocking_auth_and_headers(request, None, None, &HashMap::new(), &headers_env)
        .expect_err("headers_env value with surrounding whitespace should fail before sending");

    assert!(
        error.to_string().contains(
            "header env var name for X-Sentry-Auth must not contain surrounding whitespace"
        ),
        "unexpected error: {error}"
    );
}

#[test]
fn header_env_assignment_is_rejected_before_sending() {
    let mut headers_env = HashMap::new();
    headers_env.insert("X-Sentry-Auth".to_string(), "SENTRY_AUTH=value".to_string());
    let request = reqwest::blocking::Client::new().post("https://sentry.example.com/api/42/store/");

    let error = apply_blocking_auth_and_headers(request, None, None, &HashMap::new(), &headers_env)
        .expect_err("headers_env assignment should fail before sending");

    assert!(
        error
            .to_string()
            .contains("header env var name for X-Sentry-Auth must be an environment variable name"),
        "unexpected error: {error}"
    );
}

#[test]
fn blank_token_env_is_rejected_before_sending_outbound_requests() {
    let request =
        reqwest::blocking::Client::new().post("https://in.logs.betterstack.com/source-token");

    let error = apply_blocking_auth_and_headers(
        request,
        None,
        Some("   "),
        &HashMap::new(),
        &HashMap::new(),
    )
    .expect_err("blank token_env should fail before sending");

    assert!(
        error.to_string().contains("token env var name is empty"),
        "unexpected error: {error}"
    );
}

#[test]
fn token_env_with_surrounding_whitespace_is_rejected_before_sending() {
    let request =
        reqwest::blocking::Client::new().post("https://in.logs.betterstack.com/source-token");

    let error = apply_blocking_auth_and_headers(
        request,
        None,
        Some(" NEO_OBSERVABILITY_TOKEN "),
        &HashMap::new(),
        &HashMap::new(),
    )
    .expect_err("token_env with surrounding whitespace should fail before sending");

    assert!(
        error
            .to_string()
            .contains("token env var name must not contain surrounding whitespace"),
        "unexpected error: {error}"
    );
}

#[test]
fn token_env_assignment_is_rejected_before_sending() {
    let request =
        reqwest::blocking::Client::new().post("https://in.logs.betterstack.com/source-token");

    let error = apply_blocking_auth_and_headers(
        request,
        None,
        Some("NEO_OBSERVABILITY_TOKEN=value"),
        &HashMap::new(),
        &HashMap::new(),
    )
    .expect_err("token_env assignment should fail before sending");

    assert!(
        error
            .to_string()
            .contains("token env var name must be an environment variable name"),
        "unexpected error: {error}"
    );
}

#[test]
fn blank_inline_token_is_rejected_before_sending() {
    let request =
        reqwest::blocking::Client::new().post("https://in.logs.betterstack.com/source-token");

    let error = apply_blocking_auth_and_headers(
        request,
        Some("   "),
        None,
        &HashMap::new(),
        &HashMap::new(),
    )
    .expect_err("blank inline token should fail before sending");

    assert!(
        error.to_string().contains("token must not be empty"),
        "unexpected error: {error}"
    );
}

#[test]
fn inline_token_with_surrounding_whitespace_is_rejected_before_sending() {
    let request =
        reqwest::blocking::Client::new().post("https://in.logs.betterstack.com/source-token");

    let error = apply_blocking_auth_and_headers(
        request,
        Some(" secret "),
        None,
        &HashMap::new(),
        &HashMap::new(),
    )
    .expect_err("inline token with surrounding whitespace should fail before sending");

    assert!(
        error
            .to_string()
            .contains("token must not contain surrounding whitespace"),
        "unexpected error: {error}"
    );
}
