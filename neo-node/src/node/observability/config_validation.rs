//! Startup validation for outbound observability configuration.

use super::super::config::ObservabilitySection;
use super::endpoints::normalized_kind;

pub(super) fn validate_runtime_config(config: &ObservabilitySection) -> anyhow::Result<()> {
    validate_runtime_destinations(config)?;
    validate_runtime_send_attempts(config)?;
    validate_runtime_error_endpoint_kinds(config)?;
    validate_runtime_heartbeat_intervals(config)?;
    validate_runtime_heartbeat_methods(config)
}

fn validate_runtime_send_attempts(config: &ObservabilitySection) -> anyhow::Result<()> {
    if config.max_send_attempts == 0 {
        anyhow::bail!("[observability].max_send_attempts must be at least 1");
    }
    Ok(())
}

fn validate_runtime_destinations(config: &ObservabilitySection) -> anyhow::Result<()> {
    let enabled_error_endpoints = config
        .error_endpoints
        .iter()
        .filter(|endpoint| endpoint.enabled)
        .count();
    let enabled_heartbeat_endpoints = config
        .heartbeat_endpoints
        .iter()
        .filter(|endpoint| endpoint.enabled)
        .count();

    if enabled_error_endpoints == 0 && enabled_heartbeat_endpoints == 0 {
        anyhow::bail!(
            "[observability].enabled requires at least one enabled error or heartbeat endpoint"
        );
    }
    if config.capture_panics && enabled_error_endpoints == 0 {
        anyhow::bail!(
            "[observability].capture_panics requires at least one enabled error endpoint"
        );
    }
    Ok(())
}

fn validate_runtime_heartbeat_intervals(config: &ObservabilitySection) -> anyhow::Result<()> {
    if config.heartbeat_interval_seconds == 0 {
        anyhow::bail!("[observability].heartbeat_interval_seconds must be greater than zero");
    }
    if config
        .heartbeat_endpoints
        .iter()
        .any(|endpoint| endpoint.enabled && endpoint.interval_seconds == Some(0))
    {
        anyhow::bail!(
            "[[observability.heartbeat_endpoints]].interval_seconds must be greater than zero"
        );
    }
    Ok(())
}

fn validate_runtime_error_endpoint_kinds(config: &ObservabilitySection) -> anyhow::Result<()> {
    for endpoint in config
        .error_endpoints
        .iter()
        .filter(|endpoint| endpoint.enabled)
    {
        match normalized_kind(endpoint.kind.as_deref()).as_str() {
            "custom_json" | "better_stack_logs" | "google_error_reporting" | "sentry" => {}
            _ => anyhow::bail!(
                "[[observability.error_endpoints]].kind must be one of custom_json, better_stack_logs, google_error_reporting, or sentry"
            ),
        }
    }
    Ok(())
}

fn validate_runtime_heartbeat_methods(config: &ObservabilitySection) -> anyhow::Result<()> {
    for endpoint in config
        .heartbeat_endpoints
        .iter()
        .filter(|endpoint| endpoint.enabled)
    {
        match endpoint
            .method
            .as_deref()
            .unwrap_or("GET")
            .to_ascii_uppercase()
            .as_str()
        {
            "GET" | "POST" | "PUT" => {}
            _ => anyhow::bail!(
                "[[observability.heartbeat_endpoints]].method must be one of GET, POST, or PUT"
            ),
        }
    }
    Ok(())
}
