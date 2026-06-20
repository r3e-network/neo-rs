use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use tracing::info;

use super::*;

pub(in crate::node) fn validate_config(config: &NodeConfig, network: u32) -> anyhow::Result<()> {
    let _ = storage_backend_name(config)?;
    let _ = config.p2p.channels_config()?;
    validate_indexer_config(config, network)?;
    validate_service_store_paths(config, network)?;
    validate_telemetry_config(&config.telemetry)?;
    validate_logging_config(&config.logging)?;
    validate_observability_config(&config.observability)?;

    if let Some(bind_address) = config.p2p.bind_address.as_deref() {
        bind_address
            .parse::<std::net::IpAddr>()
            .context("invalid [p2p].bind_address")?;
    }

    if config.rpc.enabled {
        let _ = config.rpc.server_config(network)?;
    }

    Ok(())
}

fn validate_service_store_paths(config: &NodeConfig, network: u32) -> anyhow::Result<()> {
    if config.state_service.enabled {
        if let Some(path) = &config.state_service.path {
            let path = network_scoped_path(path, network);
            validate_non_empty_service_path(&path, "[state_service].path")?;
        }
    }

    if config.application_logs.enabled {
        let settings = config.application_logs.settings(network);
        validate_non_empty_service_path(Path::new(&settings.path), "[application_logs].path")?;
    }

    if config.tokens_tracker.enabled {
        let settings = config.tokens_tracker.settings(network);
        validate_non_empty_service_path(Path::new(&settings.db_path), "[tokens_tracker].db_path")?;
    }

    Ok(())
}

fn validate_non_empty_service_path(path: &Path, label: &str) -> anyhow::Result<()> {
    if path.as_os_str().is_empty() {
        anyhow::bail!("{label} must not be empty when the service is enabled");
    }
    if path.is_file() {
        anyhow::bail!(
            "{label} must be a service-store directory, got file {}",
            path.display()
        );
    }
    Ok(())
}

fn validate_telemetry_config(config: &TelemetrySection) -> anyhow::Result<()> {
    if !config.metrics.enabled {
        return Ok(());
    }
    let _ = config.metrics.bind_socket_addr()?;
    let path = config.metrics.endpoint_path();
    if !path.starts_with('/') {
        anyhow::bail!("[telemetry.metrics].path must start with '/'");
    }
    if path.contains('?') || path.contains('#') {
        anyhow::bail!("[telemetry.metrics].path must not contain query or fragment");
    }
    if [TELEMETRY_HEALTH_PATH, TELEMETRY_READY_PATH].contains(&path) {
        anyhow::bail!(
            "[telemetry.metrics].path {path:?} is reserved for the built-in health endpoint"
        );
    }
    Ok(())
}

fn validate_logging_config(config: &LoggingSection) -> anyhow::Result<()> {
    if let Some(format) = config.format.as_deref() {
        match format.trim().to_ascii_lowercase().as_str() {
            "" | "pretty" | "compact" | "json" => {}
            other => {
                anyhow::bail!(
                    "unsupported [logging].format {other:?}; expected pretty, compact, or json"
                );
            }
        }
    }
    if let Some(path) = &config.file_path {
        if path.as_os_str().is_empty() {
            anyhow::bail!("[logging].file_path must not be empty");
        }
        if path.is_dir() {
            anyhow::bail!("[logging].file_path must be a file path, got directory");
        }
    }
    if let Some(size) = config.max_file_size_bytes()? {
        if size == 0 {
            anyhow::bail!("[logging].max_file_size must be greater than zero");
        }
        if config.file_path.is_none() {
            anyhow::bail!("[logging].max_file_size requires [logging].file_path");
        }
    }
    if config.max_files == Some(0) {
        anyhow::bail!("[logging].max_files must be greater than zero");
    }
    Ok(())
}

fn validate_observability_config(config: &ObservabilitySection) -> anyhow::Result<()> {
    if !config.enabled {
        return Ok(());
    }

    if config.request_timeout_ms == 0 {
        anyhow::bail!("[observability].request_timeout_ms must be greater than zero");
    }
    if config.heartbeat_interval_seconds == 0 {
        anyhow::bail!("[observability].heartbeat_interval_seconds must be greater than zero");
    }

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

    for endpoint in config
        .error_endpoints
        .iter()
        .filter(|endpoint| endpoint.enabled)
    {
        let has_token = validate_observability_token_fields(
            endpoint.token.as_deref(),
            endpoint.token_env.as_deref(),
            "[[observability.error_endpoints]]",
        )?;
        let kind = normalized_observability_kind(endpoint.kind.as_deref());
        match kind.as_str() {
            "custom_json" | "better_stack_logs" | "sentry" => {
                validate_observability_url(
                    endpoint.url.as_deref(),
                    "[[observability.error_endpoints]].url",
                )?;
                if kind == "better_stack_logs" && !has_token {
                    anyhow::bail!(
                        "[[observability.error_endpoints]] kind=better_stack_logs requires token or token_env"
                    );
                }
            }
            "google_error_reporting" => {
                let has_url = endpoint
                    .url
                    .as_deref()
                    .is_some_and(|url| !url.trim().is_empty());
                let has_project_id = endpoint
                    .project_id
                    .as_deref()
                    .is_some_and(|project_id| !project_id.trim().is_empty());
                if !has_url && !has_project_id {
                    anyhow::bail!(
                        "[[observability.error_endpoints]] kind=google_error_reporting requires project_id or url"
                    );
                }
                if !has_url && !has_token {
                    anyhow::bail!(
                        "[[observability.error_endpoints]] kind=google_error_reporting requires token or token_env when project_id is used without url"
                    );
                }
                if has_url {
                    validate_observability_url(
                        endpoint.url.as_deref(),
                        "[[observability.error_endpoints]].url",
                    )?;
                }
            }
            other => {
                anyhow::bail!(
                    "unsupported [[observability.error_endpoints]].kind {other:?}; expected custom_json, better_stack_logs, google_error_reporting, or sentry"
                );
            }
        }

        validate_observability_headers(
            &endpoint.headers,
            &endpoint.headers_env,
            has_token,
            "[[observability.error_endpoints]]",
        )?;
    }

    for endpoint in config
        .heartbeat_endpoints
        .iter()
        .filter(|endpoint| endpoint.enabled)
    {
        let has_token = validate_observability_token_fields(
            endpoint.token.as_deref(),
            endpoint.token_env.as_deref(),
            "[[observability.heartbeat_endpoints]]",
        )?;
        validate_observability_url(
            endpoint.url.as_deref(),
            "[[observability.heartbeat_endpoints]].url",
        )?;
        if endpoint.interval_seconds == Some(0) {
            anyhow::bail!(
                "[[observability.heartbeat_endpoints]].interval_seconds must be greater than zero"
            );
        }
        match endpoint
            .method
            .as_deref()
            .unwrap_or("GET")
            .to_ascii_uppercase()
            .as_str()
        {
            "GET" | "POST" | "PUT" => {}
            method => {
                anyhow::bail!(
                    "unsupported [[observability.heartbeat_endpoints]].method {method:?}; expected GET, POST, or PUT"
                );
            }
        }
        validate_observability_headers(
            &endpoint.headers,
            &endpoint.headers_env,
            has_token,
            "[[observability.heartbeat_endpoints]]",
        )?;
    }

    Ok(())
}

fn normalized_observability_kind(kind: Option<&str>) -> String {
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

fn validate_observability_url(url: Option<&str>, label: &str) -> anyhow::Result<()> {
    let url = url.unwrap_or_default();
    let trimmed = url.trim();
    if trimmed != url {
        anyhow::bail!("{label} must not contain surrounding whitespace");
    }
    if url.is_empty() {
        anyhow::bail!("{label} must not be empty");
    }
    let parsed = url::Url::parse(url).with_context(|| format!("invalid {label}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        scheme => anyhow::bail!("invalid {label}: unsupported URL scheme {scheme:?}"),
    }
}

fn validate_observability_token_fields(
    token: Option<&str>,
    token_env: Option<&str>,
    label: &str,
) -> anyhow::Result<bool> {
    let token_configured = match token {
        Some(token) if token.trim().is_empty() => {
            anyhow::bail!("{label}.token must not be empty when set")
        }
        Some(token) if token.trim() != token => {
            anyhow::bail!("{label}.token must not contain surrounding whitespace")
        }
        Some(_) => true,
        None => false,
    };
    let token_env_configured = match token_env {
        Some(token_env) if token_env.trim().is_empty() => {
            anyhow::bail!("{label}.token_env must not be empty when set")
        }
        Some(token_env) if token_env.trim() != token_env => {
            anyhow::bail!("{label}.token_env must not contain surrounding whitespace")
        }
        Some(token_env) if token_env.contains('=') => {
            anyhow::bail!("{label}.token_env must be an environment variable name")
        }
        Some(_) => true,
        None => false,
    };
    Ok(token_configured || token_env_configured)
}

fn validate_observability_headers(
    headers: &HashMap<String, String>,
    headers_env: &HashMap<String, String>,
    has_token: bool,
    label: &str,
) -> anyhow::Result<()> {
    let mut has_authorization_header = false;
    for (key, value) in headers {
        validate_observability_header_name(key, &format!("{label}.headers"))?;
        if key.eq_ignore_ascii_case("authorization") {
            has_authorization_header = true;
        }
        reqwest::header::HeaderValue::from_str(value).with_context(|| {
            format!("{label}.headers contains invalid HTTP header value for {key:?}")
        })?;
    }
    for (key, env_var) in headers_env {
        validate_observability_header_name(key, &format!("{label}.headers_env"))?;
        if key.eq_ignore_ascii_case("authorization") {
            has_authorization_header = true;
        }
        if headers
            .keys()
            .any(|static_key| static_key.eq_ignore_ascii_case(key))
        {
            anyhow::bail!(
                "{label} must not configure the same header in both headers and headers_env: {key:?}"
            );
        }
        validate_observability_env_var_name(env_var, &format!("{label}.headers_env.{key}"))?;
    }
    if has_token && has_authorization_header {
        anyhow::bail!(
            "{label}.headers must not include Authorization when token or token_env is configured"
        );
    }
    Ok(())
}

fn validate_observability_header_name(key: &str, label: &str) -> anyhow::Result<()> {
    if key.trim().is_empty() {
        anyhow::bail!("{label} must not contain empty header names");
    }
    if key.trim() != key {
        anyhow::bail!("{label} key {key:?} must not contain surrounding whitespace");
    }
    reqwest::header::HeaderName::from_bytes(key.as_bytes())
        .with_context(|| format!("{label} contains invalid HTTP header name {key:?}"))?;
    Ok(())
}

fn validate_observability_env_var_name(env_var: &str, label: &str) -> anyhow::Result<()> {
    if env_var.trim().is_empty() {
        anyhow::bail!("{label} must not be empty when set");
    }
    if env_var.trim() != env_var {
        anyhow::bail!("{label} must not contain surrounding whitespace");
    }
    if env_var.contains('=') {
        anyhow::bail!("{label} must be an environment variable name");
    }
    Ok(())
}

fn validate_indexer_config(config: &NodeConfig, network: u32) -> anyhow::Result<()> {
    if !config.indexer.enabled {
        return Ok(());
    }
    if config.indexer.path.is_some() && config.indexer.store_path.is_some() {
        anyhow::bail!(
            "[indexer].path and [indexer].store_path are mutually exclusive; use store_path for the RocksDB-backed service indexer"
        );
    }
    if let Some(path) = &config.indexer.store_path {
        let path = network_scoped_path(path, network);
        if path.as_os_str().is_empty() {
            anyhow::bail!("[indexer].store_path must not be empty when [indexer].enabled is true");
        }
        if path.is_file() {
            anyhow::bail!(
                "[indexer].store_path must be a service-store directory, got file {}",
                path.display()
            );
        }
    }
    let Some(path) = &config.indexer.path else {
        return Ok(());
    };

    let path = network_scoped_path(path, network);
    if path.as_os_str().is_empty() {
        anyhow::bail!("[indexer].path must not be empty when [indexer].enabled is true");
    }
    if path.is_dir() {
        anyhow::bail!(
            "[indexer].path must be a JSON snapshot file, got directory {}",
            path.display()
        );
    }
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        anyhow::bail!(
            "[indexer].path must be a JSON snapshot file ending in .json, got {}",
            path.display()
        );
    }

    Ok(())
}

pub(in crate::node) fn validate_storage(
    config: &NodeConfig,
    storage_override: Option<&Path>,
) -> anyhow::Result<()> {
    let _store = open_store(config, storage_override)?;
    Ok(())
}

fn storage_backend_name(config: &NodeConfig) -> anyhow::Result<&str> {
    let backend = config.storage.backend.as_deref().unwrap_or("memory");
    if backend.eq_ignore_ascii_case("memory") || backend.eq_ignore_ascii_case("rocksdb") {
        Ok(backend)
    } else {
        anyhow::bail!(
            "unsupported [storage].backend {backend:?}; expected \"memory\" or \"rocksdb\""
        );
    }
}

pub(in crate::node) fn open_store(
    config: &NodeConfig,
    storage_override: Option<&Path>,
) -> anyhow::Result<Arc<dyn neo_storage::persistence::store::Store>> {
    use neo_storage::persistence::StoreProvider;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::storage::StorageConfig;
    use neo_storage::persistence::store::Store;
    use neo_storage::rocksdb::RocksDBStoreProvider;

    let backend = storage_backend_name(config)?;
    let use_rocksdb = storage_override.is_some()
        || backend.eq_ignore_ascii_case("rocksdb")
        || config.storage.read_only;
    let store: Arc<dyn Store> = if use_rocksdb {
        let path = storage_override
            .map(Path::to_path_buf)
            .or_else(|| config.storage.data_directory())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "storage backend 'rocksdb' requires a data directory \
                     (set [storage].data_dir or [storage].path, or pass --storage-path)"
                )
            })?;
        info!(target: "neo", path = %path.display(), "opening RocksDB store");
        let cfg = StorageConfig {
            path,
            read_only: config.storage.read_only,
            ..Default::default()
        };
        RocksDBStoreProvider::new(cfg)
            .get_store("")
            .map_err(|e| anyhow::anyhow!("failed to open RocksDB store: {e}"))?
    } else {
        info!(target: "neo", "using in-memory store (state is not persisted across restarts)");
        Arc::new(MemoryStore::new())
    };

    Ok(store)
}
