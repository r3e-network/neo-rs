//! Endpoint naming, URL resolution, and outbound request authentication.

use std::collections::HashMap;

use anyhow::Context;

use super::super::config::{ObservabilityErrorEndpoint, ObservabilityHeartbeatEndpoint};

pub(super) fn error_endpoint_url(
    endpoint: &ObservabilityErrorEndpoint,
    kind: &str,
) -> anyhow::Result<String> {
    if let Some(raw_url) = endpoint.url.as_deref() {
        let url = raw_url.trim();
        if url != raw_url {
            anyhow::bail!("endpoint url must not contain surrounding whitespace");
        }
        if !url.is_empty() {
            return Ok(raw_url.to_string());
        }
    }

    if kind == "google_error_reporting" {
        let project_id = endpoint
            .project_id
            .as_deref()
            .map(str::trim)
            .filter(|project_id| !project_id.is_empty())
            .context("google_error_reporting endpoint requires project_id or url")?;
        return Ok(format!(
            "https://clouderrorreporting.googleapis.com/v1beta1/projects/{project_id}/events:report"
        ));
    }

    anyhow::bail!("{} endpoint requires url", error_endpoint_name(endpoint))
}

pub(super) fn heartbeat_endpoint_url(
    endpoint: &ObservabilityHeartbeatEndpoint,
) -> anyhow::Result<&str> {
    let raw_url = endpoint.url.as_deref().context("heartbeat URL is empty")?;
    let url = raw_url.trim();
    if url.is_empty() {
        anyhow::bail!("heartbeat URL is empty");
    }
    if url != raw_url {
        anyhow::bail!("heartbeat URL must not contain surrounding whitespace");
    }
    Ok(raw_url)
}

pub(super) fn apply_blocking_auth_and_headers(
    mut request: reqwest::blocking::RequestBuilder,
    token: Option<&str>,
    token_env: Option<&str>,
    headers: &HashMap<String, String>,
    headers_env: &HashMap<String, String>,
) -> anyhow::Result<reqwest::blocking::RequestBuilder> {
    if let Some(token) = resolved_token(token, token_env)? {
        request = request.bearer_auth(token);
    }
    for (key, value) in headers {
        request = request.header(key.as_str(), value.as_str());
    }
    for (key, env_var) in headers_env {
        request = request.header(key.as_str(), resolved_header_env_value(key, env_var)?);
    }
    Ok(request)
}

pub(super) fn apply_async_auth_and_headers(
    mut request: reqwest::RequestBuilder,
    token: Option<&str>,
    token_env: Option<&str>,
    headers: &HashMap<String, String>,
    headers_env: &HashMap<String, String>,
) -> anyhow::Result<reqwest::RequestBuilder> {
    if let Some(token) = resolved_token(token, token_env)? {
        request = request.bearer_auth(token);
    }
    for (key, value) in headers {
        request = request.header(key.as_str(), value.as_str());
    }
    for (key, env_var) in headers_env {
        request = request.header(key.as_str(), resolved_header_env_value(key, env_var)?);
    }
    Ok(request)
}

fn resolved_token(token: Option<&str>, token_env: Option<&str>) -> anyhow::Result<Option<String>> {
    if let Some(token) = token {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            anyhow::bail!("token must not be empty");
        }
        if trimmed != token {
            anyhow::bail!("token must not contain surrounding whitespace");
        }
        return Ok(Some(token.to_string()));
    }
    let Some(raw_var_name) = token_env else {
        return Ok(None);
    };
    let var_name = raw_var_name.trim();
    if var_name.is_empty() {
        anyhow::bail!("token env var name is empty");
    }
    if var_name != raw_var_name {
        anyhow::bail!("token env var name must not contain surrounding whitespace");
    }
    if var_name.contains('=') {
        anyhow::bail!("token env var name must be an environment variable name");
    }
    match std::env::var(var_name) {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value)),
        Ok(_) => anyhow::bail!("token env var {var_name} is empty"),
        Err(std::env::VarError::NotPresent) => {
            anyhow::bail!("token env var {var_name} is not set")
        }
        Err(err) => Err(err).with_context(|| format!("reading token env var {var_name}")),
    }
}

fn resolved_header_env_value(header_name: &str, env_var: &str) -> anyhow::Result<String> {
    let var_name = env_var.trim();
    if var_name.is_empty() {
        anyhow::bail!("header env var name for {header_name} is empty");
    }
    if var_name != env_var {
        anyhow::bail!(
            "header env var name for {header_name} must not contain surrounding whitespace"
        );
    }
    if var_name.contains('=') {
        anyhow::bail!("header env var name for {header_name} must be an environment variable name");
    }
    match std::env::var(var_name) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        Ok(_) => anyhow::bail!("header env var {var_name} for {header_name} is empty"),
        Err(std::env::VarError::NotPresent) => {
            anyhow::bail!("header env var {var_name} for {header_name} is not set")
        }
        Err(err) => {
            Err(err).with_context(|| format!("reading header env var {var_name} for {header_name}"))
        }
    }
}

pub(super) fn normalized_kind(kind: Option<&str>) -> String {
    match kind
        .unwrap_or("custom_json")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "custom" | "generic_json" | "custom_json" => "custom_json".to_string(),
        "betterstack" | "better_stack" | "better_stack_logs" => "better_stack_logs".to_string(),
        "google" | "gcp" | "google_error_reporting" => "google_error_reporting".to_string(),
        "sentry" | "sentry_store" | "sentry_error_reporting" => "sentry".to_string(),
        other => other.to_string(),
    }
}

pub(super) fn error_endpoint_name(endpoint: &ObservabilityErrorEndpoint) -> String {
    endpoint
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .or(endpoint.kind.as_deref())
        .unwrap_or("custom_json")
        .to_string()
}

pub(super) fn heartbeat_endpoint_name(endpoint: &ObservabilityHeartbeatEndpoint) -> String {
    if let Some(name) = endpoint
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        return name.to_string();
    }
    // Never fall back to the raw URL: heartbeat URLs embed a secret token in the
    // path (e.g. Better Stack `.../heartbeat/<id>`) that would otherwise leak into
    // logs and into error reports forwarded to other providers. Use a redacted
    // scheme://host label instead.
    match endpoint.url.as_deref() {
        Some(url) => redact_url(url),
        None => "heartbeat".to_string(),
    }
}

/// Returns a log-safe label for a URL: `scheme://host[:port]`, with any userinfo
/// credentials and the secret-bearing path/query/fragment removed. Used wherever a
/// destination URL would otherwise be written into logs or outbound error reports.
pub(super) fn redact_url(url: &str) -> String {
    let url = url.trim();
    let Some((scheme, rest)) = url.split_once("://") else {
        return "<redacted-url>".to_string();
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let host = match authority.rsplit_once('@') {
        Some((_credentials, host)) => host,
        None => authority,
    };
    if host.is_empty() {
        return "<redacted-url>".to_string();
    }
    format!("{scheme}://{host}")
}

pub(super) fn trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn trimmed_or_default(value: Option<&str>, default: &str) -> String {
    trimmed(value).unwrap_or_else(|| default.to_string())
}
