use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use tracing::info;

use super::super::cli::LedgerMode;
use super::super::ledger_source::store_ledger_index;
use super::*;

#[cfg(test)]
pub(in crate::node) fn validate_config(config: &NodeConfig, network: u32) -> anyhow::Result<()> {
    validate_config_with_local_replay_services(config, network, true, None)
}

pub(in crate::node) fn validate_config_for_ledger_mode(
    config: &NodeConfig,
    network: u32,
    ledger_mode: LedgerMode<'_>,
    storage_override: Option<&Path>,
) -> anyhow::Result<()> {
    validate_config_with_local_replay_services(
        config,
        network,
        ledger_mode.uses_local_replay_services(),
        storage_override,
    )
}

fn validate_config_with_local_replay_services(
    config: &NodeConfig,
    network: u32,
    validate_local_replay_services: bool,
    storage_override: Option<&Path>,
) -> anyhow::Result<()> {
    let _ = storage_backend_name(config)?;
    let _ = config.p2p.channels_config()?;
    if validate_local_replay_services {
        validate_static_files_config(config, storage_override)?;
        validate_append_shadow_config(config, network, storage_override)?;
        validate_state_packs_config(config, network, storage_override)?;
        validate_indexer_config(config, network)?;
        validate_service_store_paths(config, network)?;
    }
    validate_telemetry_config(&config.telemetry)?;
    validate_logging_config(&config.logging)?;
    validate_observability_config(&config.observability)?;
    config
        .execution
        .specialization_shadow
        .validate(validate_local_replay_services)?;
    config
        .execution
        .optimistic_signature_verification
        .validate(validate_local_replay_services)?;

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

fn validate_static_files_config(
    config: &NodeConfig,
    storage_override: Option<&Path>,
) -> anyhow::Result<()> {
    let Some(directory) = &config.storage.static_files_dir else {
        return Ok(());
    };
    if directory.as_os_str().is_empty() {
        anyhow::bail!("[storage].static_files_dir must not be empty");
    }
    if directory.is_file() {
        anyhow::bail!(
            "[storage].static_files_dir must be a directory, got file {}",
            directory.display()
        );
    }
    if persistent_store_provider(config, storage_override)?.is_none() {
        anyhow::bail!("[storage].static_files_dir requires the durable canonical MDBX backend");
    }
    if config.storage.read_only {
        anyhow::bail!(
            "[storage].static_files_dir cannot be enabled while [storage].read_only is true"
        );
    }
    config
        .storage
        .static_file_config()
        .validate()
        .map_err(|error| anyhow::anyhow!("invalid static-file configuration: {error}"))?;
    if config.storage.static_file_recovery_batch_blocks() == 0 {
        anyhow::bail!("[storage].static_files_recovery_batch_blocks must be greater than zero");
    }
    Ok(())
}

fn validate_append_shadow_config(
    config: &NodeConfig,
    network: u32,
    storage_override: Option<&Path>,
) -> anyhow::Result<()> {
    let shadow = &config.storage.append_shadow;
    if !shadow.enabled {
        return Ok(());
    }
    let Some(path) = shadow.path.as_deref() else {
        anyhow::bail!("[storage.append_shadow].path is required when the shadow is enabled");
    };
    let path = network_scoped_path(path, network);
    validate_non_empty_service_path(&path, "[storage.append_shadow].path")?;
    if persistent_store_provider(config, storage_override)?.is_none() {
        anyhow::bail!("[storage.append_shadow] requires the durable canonical MDBX backend");
    }
    if config.storage.read_only {
        anyhow::bail!(
            "[storage.append_shadow] cannot be enabled while [storage].read_only is true"
        );
    }
    if !config.state_service.enabled || !config.state_service.coordinated {
        anyhow::bail!(
            "[storage.append_shadow] requires [state_service].enabled=true with coordinated commits"
        );
    }
    if shadow.max_index_memory_mb == Some(0) {
        anyhow::bail!("[storage.append_shadow].max_index_memory_mb must be greater than zero");
    }
    Ok(())
}

fn validate_state_packs_config(
    config: &NodeConfig,
    network: u32,
    storage_override: Option<&Path>,
) -> anyhow::Result<()> {
    let packs = &config.storage.state_packs;
    if !packs.enabled {
        return Ok(());
    }
    let Some(path) = packs.path.as_deref() else {
        anyhow::bail!("[storage.state_packs].path is required when authority is enabled");
    };
    let path = network_scoped_path(path, network);
    validate_non_empty_service_path(&path, "[storage.state_packs].path")?;
    if persistent_store_provider(config, storage_override)?.is_none() {
        anyhow::bail!("[storage.state_packs] requires the durable canonical MDBX backend");
    }
    if config.storage.read_only {
        anyhow::bail!("[storage.state_packs] cannot be enabled with read-only storage");
    }
    if config.storage.append_shadow.enabled {
        anyhow::bail!(
            "authoritative [storage.state_packs] and append shadow are mutually exclusive"
        );
    }
    if !config.state_service.enabled
        || !config.state_service.coordinated
        || !config.state_service.full_state
        || !config.state_service.track_during_catchup
    {
        anyhow::bail!(
            "[storage.state_packs] requires coordinated full-state StateService tracking during catch-up"
        );
    }
    if packs.max_index_memory_mb == Some(0) {
        anyhow::bail!("[storage.state_packs].max_index_memory_mb must be greater than zero");
    }
    if !(1..=8).contains(&packs.batch_value_workers()) {
        anyhow::bail!("[storage.state_packs].batch_value_workers must be in 1..=8");
    }
    Ok(())
}

fn validate_service_store_paths(config: &NodeConfig, network: u32) -> anyhow::Result<()> {
    if config.state_service.enabled {
        if config.state_service.defer_full_state_finalization && !config.state_service.full_state {
            anyhow::bail!(
                "[state_service].defer_full_state_finalization requires [state_service].full_state=true"
            );
        }
        match (&config.state_service.path, config.state_service.coordinated) {
            (Some(path), _) => {
                let path = network_scoped_path(path, network);
                validate_non_empty_service_path(&path, "[state_service].path")?;
            }
            (None, false) => anyhow::bail!(
                "[state_service].path is required when [state_service].coordinated=false"
            ),
            (None, true) => {}
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
    Ok(())
}

pub(in crate::node) fn validate_storage(
    config: &NodeConfig,
    storage_override: Option<&Path>,
    network: u32,
) -> anyhow::Result<()> {
    let storage_root = storage_override
        .map(Path::to_path_buf)
        .or_else(|| config.storage.data_directory());
    let marker_path = crate::node::recovery::local_replay_marker_path(storage_root.as_deref());
    crate::node::recovery::refuse_local_replay_marker(marker_path.as_deref())?;

    let state_service_provider = service_store_provider(config)?;
    let store = open_store(config, storage_override)?;
    let ledger_index = durable_ledger_index(&store)?;
    validate_state_service_storage(
        config,
        network,
        ledger_index,
        &state_service_provider,
        &store,
    )?;
    Ok(())
}

fn durable_ledger_index<S>(store: &Arc<S>) -> anyhow::Result<Option<u32>>
where
    S: neo_storage::persistence::store::Store + 'static,
{
    store_ledger_index(store, false)
}

pub(in crate::node) fn validate_state_service_storage(
    config: &NodeConfig,
    network: u32,
    ledger_index: Option<u32>,
    storage_provider: &str,
    canonical_store: &Arc<neo_storage::persistence::providers::RuntimeStore>,
) -> anyhow::Result<()> {
    if !config.state_service.enabled {
        return Ok(());
    }
    let chain_height = ledger_index;
    if config.state_service.coordinated
        && storage_provider.eq_ignore_ascii_case("mdbx")
        && canonical_store.as_mdbx().is_some()
    {
        let state_store = canonical_store
            .open_coordinated_namespace(neo_state_service::MDBX_STATE_SERVICE_NAMESPACE)
            .context("opening coordinated StateService MDBX namespace for validation")?;
        let label = format!(
            "canonical MDBX namespace {}",
            neo_state_service::MDBX_STATE_SERVICE_NAMESPACE
        );
        let state_height = read_state_service_height_from_store(&state_store, &label)?;
        return validate_state_service_height(chain_height, state_height, &label);
    }

    let Some(path) = &config.state_service.path else {
        return match chain_height {
            Some(chain_height) => anyhow::bail!(
                "StateService is enabled while the chain store is already at height {chain_height}, but [state_service].path is not configured; set a persisted StateRoot path, restore a matching checkpoint, or replay from genesis with [state_service].track_during_catchup = true"
            ),
            None => Ok(()),
        };
    };

    let path = network_scoped_path(path, network);
    if !path.exists() {
        return match chain_height {
            Some(chain_height) => anyhow::bail!(
                "StateService MPT store {} is missing while the chain store is already at height {chain_height}; restore a matching StateRoot checkpoint or replay from genesis with [state_service].track_during_catchup = true",
                path.display()
            ),
            None => Ok(()),
        };
    }

    let state_height = read_state_service_mpt_height(storage_provider, &config.storage, &path)?;
    validate_state_service_height(chain_height, state_height, &path.display().to_string())
}

fn validate_state_service_height(
    chain_height: Option<u32>,
    state_height: Option<u32>,
    label: &str,
) -> anyhow::Result<()> {
    let Some(chain_height) = chain_height else {
        return match state_height {
            Some(height) => anyhow::bail!(
                "StateService MPT height {height} at {label} exists while the chain store is uninitialized; restore matching canonical and StateService data or remove the local store and replay from genesis"
            ),
            None => Ok(()),
        };
    };
    match state_height {
        Some(height) if height == chain_height => Ok(()),
        Some(height) => anyhow::bail!(
            "StateService MPT height {height} at {label} does not match chain height {chain_height}; restore a matching checkpoint or replay from genesis with [state_service].track_during_catchup = true"
        ),
        None => anyhow::bail!(
            "StateService MPT store at {label} has no current local root while the chain store is already at height {chain_height}; restore a matching checkpoint or replay from genesis with [state_service].track_during_catchup = true"
        ),
    }
}

fn read_state_service_height_from_store<S>(store: &S, label: &str) -> anyhow::Result<Option<u32>>
where
    S: neo_storage::persistence::Store,
{
    use neo_storage::persistence::RawReadOnlyStore;

    const CURRENT_LOCAL_ROOT_INDEX_KEY: &[u8] = &[0x02];
    let snapshot = store.snapshot();
    let Some(value) = snapshot.try_get_bytes(CURRENT_LOCAL_ROOT_INDEX_KEY) else {
        return Ok(None);
    };
    if value.len() != 4 {
        anyhow::bail!(
            "StateService MPT current local root index at {label} is malformed: expected 4 bytes, got {}",
            value.len()
        );
    }

    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&value);
    Ok(Some(u32::from_le_bytes(bytes)))
}

fn read_state_service_mpt_height(
    storage_provider: &str,
    storage: &StorageSection,
    path: &Path,
) -> anyhow::Result<Option<u32>> {
    use neo_storage::persistence::StoreFactory;

    let mut cfg = storage.storage_config_for_path(path.to_path_buf());
    cfg.read_only = true;
    let store = StoreFactory::get_store_with_config(storage_provider, cfg).map_err(|err| {
        anyhow::anyhow!("failed to open StateService MPT {storage_provider} store: {err}")
    })?;
    read_state_service_height_from_store(store.as_ref(), &path.display().to_string())
}

fn storage_backend_name(config: &NodeConfig) -> anyhow::Result<&str> {
    let backend = config.storage.backend.as_deref().unwrap_or("memory");
    if backend.eq_ignore_ascii_case("memory") || backend.eq_ignore_ascii_case("mdbx") {
        return Ok(backend);
    }
    anyhow::bail!("{}", unsupported_storage_backend_message(backend));
}

pub(in crate::node) fn default_persistent_storage_provider() -> &'static str {
    "mdbx"
}

fn persistent_store_provider(
    config: &NodeConfig,
    storage_override: Option<&Path>,
) -> anyhow::Result<Option<String>> {
    let backend = storage_backend_name(config)?;
    let persistent_path = storage_override.is_some()
        || config.storage.data_directory().is_some()
        || config.storage.read_only;
    if backend.eq_ignore_ascii_case("memory") {
        return Ok(persistent_path.then(|| default_persistent_storage_provider().to_string()));
    }
    Ok(Some(backend.to_ascii_lowercase()))
}

pub(in crate::node) fn service_store_provider(config: &NodeConfig) -> anyhow::Result<String> {
    let backend = storage_backend_name(config)?;
    if backend.eq_ignore_ascii_case("memory") {
        Ok(default_persistent_storage_provider().to_string())
    } else {
        Ok(backend.to_ascii_lowercase())
    }
}

fn unsupported_storage_backend_message(backend: &str) -> String {
    let expected = "\"memory\" or \"mdbx\"";
    format!("unsupported [storage].backend {backend:?}; expected {expected}")
}

pub(in crate::node) fn open_store(
    config: &NodeConfig,
    storage_override: Option<&Path>,
) -> anyhow::Result<Arc<neo_storage::persistence::providers::RuntimeStore>> {
    use neo_storage::persistence::StoreFactory;

    let provider = persistent_store_provider(config, storage_override)?;
    let store = if let Some(provider) = provider {
        let path = storage_override
            .map(Path::to_path_buf)
            .or_else(|| config.storage.data_directory())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "storage backend '{provider}' requires a data directory \
                     (set [storage].data_dir or [storage].path, or pass --storage-path)"
                )
            })?;
        info!(target: "neo", backend = provider, path = %path.display(), "opening persistent store");
        let cfg = config.storage.storage_config_for_path(path.clone());
        StoreFactory::get_store_with_config(&provider, cfg)
            .map_err(|e| anyhow::anyhow!("failed to open {provider} store: {e}"))?
    } else {
        info!(target: "neo", "using in-memory store (state is not persisted across restarts)");
        open_memory_store()?
    };

    Ok(store)
}

pub(in crate::node) fn open_memory_store()
-> anyhow::Result<Arc<neo_storage::persistence::providers::RuntimeStore>> {
    use neo_storage::persistence::StoreFactory;

    StoreFactory::get_store("memory", "").map_err(anyhow::Error::from)
}
