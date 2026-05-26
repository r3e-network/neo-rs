use anyhow::{Context, Result, bail};
use neo_core::{
    UnhandledExceptionPolicy, application_logs::ApplicationLogsSettings, constants::MAX_BLOCK_SIZE,
    oracle_service::OracleServiceSettings, state_service::state_store::StateServiceSettings,
    tokens_tracker::TokensTrackerSettings,
};
use serde::Deserialize;
use std::{env, fs, path::PathBuf, time::Duration};
use url::Url;

use super::{
    ApplicationLogsSection, DbftSection, DbftSettings, OracleServiceNeoFsSection,
    OracleServiceSection, StateServiceSection, TokensTrackerSection,
};

fn plugins_directory() -> PathBuf {
    match env::var_os("NEO_PLUGINS_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from("data/Plugins"),
    }
}

pub(super) fn config_directory() -> PathBuf {
    plugins_directory().join("RpcServer")
}

fn application_logs_config_path() -> PathBuf {
    plugins_directory()
        .join("ApplicationLogs")
        .join("ApplicationLogs.json")
}

fn application_logs_directory() -> PathBuf {
    plugins_directory().join("ApplicationLogs")
}

fn state_service_config_path() -> PathBuf {
    plugins_directory()
        .join("StateService")
        .join("StateService.json")
}

fn state_service_directory() -> PathBuf {
    plugins_directory().join("StateService")
}

fn tokens_tracker_config_path() -> PathBuf {
    plugins_directory()
        .join("TokensTracker")
        .join("TokensTracker.json")
}

fn tokens_tracker_directory() -> PathBuf {
    plugins_directory().join("TokensTracker")
}

fn oracle_service_config_path() -> PathBuf {
    plugins_directory()
        .join("OracleService")
        .join("OracleService.json")
}

fn dbft_config_path() -> PathBuf {
    plugins_directory()
        .join("DBFTPlugin")
        .join("DBFTPlugin.json")
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct PluginConfig<T> {
    #[serde(rename = "PluginConfiguration")]
    plugin_configuration: Option<T>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ApplicationLogsPluginSection {
    #[serde(rename = "Path")]
    path: Option<String>,
    #[serde(rename = "Network")]
    network: Option<u32>,
    #[serde(rename = "MaxStackSize")]
    max_stack_size: Option<usize>,
    #[serde(rename = "Debug")]
    debug: Option<bool>,
    #[serde(rename = "UnhandledExceptionPolicy")]
    unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct StateServicePluginSection {
    #[serde(rename = "Path")]
    path: Option<String>,
    #[serde(rename = "FullState")]
    full_state: Option<bool>,
    #[serde(rename = "Network")]
    network: Option<u32>,
    #[serde(rename = "AutoVerify")]
    auto_verify: Option<bool>,
    #[serde(rename = "MaxFindResultItems")]
    max_find_result_items: Option<usize>,
    #[serde(rename = "UnhandledExceptionPolicy")]
    unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct TokensTrackerPluginSection {
    #[serde(rename = "DBPath")]
    db_path: Option<String>,
    #[serde(rename = "TrackHistory")]
    track_history: Option<bool>,
    #[serde(rename = "MaxResults")]
    max_results: Option<u32>,
    #[serde(rename = "Network")]
    network: Option<u32>,
    #[serde(rename = "EnabledTrackers")]
    enabled_trackers: Vec<String>,
    #[serde(rename = "UnhandledExceptionPolicy")]
    unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct OracleServicePluginSection {
    #[serde(rename = "Network")]
    network: Option<u32>,
    #[serde(rename = "Nodes")]
    nodes: Vec<String>,
    #[serde(rename = "MaxTaskTimeout")]
    max_task_timeout: Option<u64>,
    #[serde(rename = "MaxOracleTimeout")]
    max_oracle_timeout: Option<u64>,
    #[serde(rename = "AllowPrivateHost")]
    allow_private_host: Option<bool>,
    #[serde(rename = "AllowedContentTypes")]
    allowed_content_types: Vec<String>,
    #[serde(rename = "Https")]
    https: Option<super::OracleServiceHttpsSection>,
    #[serde(rename = "NeoFS")]
    neofs: Option<OracleServiceNeoFsSection>,
    #[serde(rename = "AutoStart")]
    auto_start: Option<bool>,
    #[serde(rename = "UnhandledExceptionPolicy")]
    unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct DbftPluginSection {
    #[serde(rename = "RecoveryLogs")]
    recovery_logs: Option<String>,
    #[serde(rename = "IgnoreRecoveryLogs")]
    ignore_recovery_logs: Option<bool>,
    #[serde(rename = "AutoStart")]
    auto_start: Option<bool>,
    #[serde(rename = "Network")]
    network: Option<u32>,
    #[serde(rename = "MaxBlockSize")]
    max_block_size: Option<u32>,
    #[serde(rename = "MaxBlockSystemFee")]
    max_block_system_fee: Option<i64>,
    #[serde(rename = "UnhandledExceptionPolicy")]
    unhandled_exception_policy: Option<String>,
}

macro_rules! load_plugin_section {
    ($path:expr_2021, $section_ty:ty, $name:literal) => {{
        let path = $path;
        if !path.exists() {
            Ok::<Option<_>, anyhow::Error>(None)
        } else {
            let raw = fs::read_to_string(&path).with_context(|| {
                format!("unable to read {} config at {}", $name, path.display())
            })?;
            let config: PluginConfig<$section_ty> = serde_json::from_str(&raw)
                .with_context(|| format!("invalid {} config in {}", $name, path.display()))?;
            let Some(section) = config.plugin_configuration else {
                bail!(
                    "{} config at {} missing PluginConfiguration",
                    $name,
                    path.display()
                );
            };
            Ok::<Option<_>, anyhow::Error>(Some(section))
        }
    }};
}

pub(super) fn application_logs_section_settings(
    section: &ApplicationLogsSection,
    default_network: u32,
) -> ApplicationLogsSettings {
    build_application_logs_settings(
        default_network,
        section.path.clone(),
        section.network,
        section.max_stack_size,
        section.debug,
        section.unhandled_exception_policy.clone(),
    )
}

pub(super) fn state_service_section_settings(
    section: &StateServiceSection,
    default_network: u32,
) -> StateServiceSettings {
    build_state_service_settings(
        default_network,
        section.path.clone(),
        section.full_state,
        section.network,
        section.auto_verify,
        section.max_find_result_items,
        section.unhandled_exception_policy.clone(),
    )
}

pub(super) fn tokens_tracker_section_settings(
    section: &TokensTrackerSection,
    default_network: u32,
) -> TokensTrackerSettings {
    build_tokens_tracker_settings(
        default_network,
        section.db_path.clone(),
        section.track_history,
        section.max_results,
        section.network,
        section.enabled_trackers.clone(),
        section.unhandled_exception_policy.clone(),
    )
}

pub(super) fn oracle_service_section_settings(
    section: &OracleServiceSection,
    default_network: u32,
) -> OracleServiceSettings {
    build_oracle_service_settings(
        default_network,
        section.network,
        section.nodes.clone(),
        section.max_task_timeout,
        section.max_oracle_timeout,
        section.allow_private_host,
        section.allowed_content_types.clone(),
        section.https.clone(),
        section.neofs.clone(),
        section.auto_start,
        section.unhandled_exception_policy.clone(),
    )
}

pub(super) fn dbft_section_settings(section: &DbftSection, default_network: u32) -> DbftSettings {
    build_dbft_settings(
        default_network,
        section.recovery_logs.clone(),
        section.ignore_recovery_logs,
        section.auto_start,
        section.network,
        section.max_block_size,
        section.max_block_system_fee,
        section.unhandled_exception_policy.clone(),
    )
}

pub(super) fn load_application_logs_plugin_settings(
    default_network: u32,
) -> Result<Option<ApplicationLogsSettings>> {
    let Some(section) = load_plugin_section!(
        application_logs_config_path(),
        ApplicationLogsPluginSection,
        "ApplicationLogs"
    )?
    else {
        return Ok(None);
    };
    Ok(Some(build_application_logs_settings(
        default_network,
        section.path,
        section.network,
        section.max_stack_size,
        section.debug,
        section.unhandled_exception_policy,
    )))
}

pub(super) fn load_state_service_plugin_settings(
    default_network: u32,
) -> Result<Option<StateServiceSettings>> {
    let Some(section) = load_plugin_section!(
        state_service_config_path(),
        StateServicePluginSection,
        "StateService"
    )?
    else {
        return Ok(None);
    };
    Ok(Some(build_state_service_settings(
        default_network,
        section.path,
        section.full_state,
        section.network,
        section.auto_verify,
        section.max_find_result_items,
        section.unhandled_exception_policy,
    )))
}

pub(super) fn load_tokens_tracker_plugin_settings(
    default_network: u32,
) -> Result<Option<TokensTrackerSettings>> {
    let Some(section) = load_plugin_section!(
        tokens_tracker_config_path(),
        TokensTrackerPluginSection,
        "TokensTracker"
    )?
    else {
        return Ok(None);
    };
    Ok(Some(build_tokens_tracker_settings(
        default_network,
        section.db_path,
        section.track_history,
        section.max_results,
        section.network,
        section.enabled_trackers,
        section.unhandled_exception_policy,
    )))
}

pub(super) fn load_oracle_service_plugin_settings(
    default_network: u32,
) -> Result<Option<OracleServiceSettings>> {
    let Some(section) = load_plugin_section!(
        oracle_service_config_path(),
        OracleServicePluginSection,
        "OracleService"
    )?
    else {
        return Ok(None);
    };
    Ok(Some(build_oracle_service_settings(
        default_network,
        section.network,
        section.nodes,
        section.max_task_timeout,
        section.max_oracle_timeout,
        section.allow_private_host,
        section.allowed_content_types,
        section.https,
        section.neofs,
        section.auto_start,
        section.unhandled_exception_policy,
    )))
}

pub(super) fn load_dbft_plugin_settings(default_network: u32) -> Result<Option<DbftSettings>> {
    let Some(section) = load_plugin_section!(dbft_config_path(), DbftPluginSection, "DBFTPlugin")?
    else {
        return Ok(None);
    };
    Ok(Some(build_dbft_settings(
        default_network,
        section.recovery_logs,
        section.ignore_recovery_logs,
        section.auto_start,
        section.network,
        section.max_block_size,
        section.max_block_system_fee,
        section.unhandled_exception_policy,
    )))
}

pub fn resolve_application_logs_store_path(settings: &ApplicationLogsSettings) -> PathBuf {
    let path = PathBuf::from(format_network_path(&settings.path, settings.network));
    if path.is_absolute() {
        path
    } else {
        application_logs_directory().join(path)
    }
}

pub fn resolve_state_service_store_path(settings: &StateServiceSettings) -> PathBuf {
    let path = PathBuf::from(format_network_path(&settings.path, settings.network));
    if path.is_absolute() {
        path
    } else {
        state_service_directory().join(path)
    }
}

pub fn resolve_tokens_tracker_store_path(settings: &TokensTrackerSettings) -> PathBuf {
    let path = PathBuf::from(format_network_path(&settings.db_path, settings.network));
    if path.is_absolute() {
        path
    } else {
        tokens_tracker_directory().join(path)
    }
}

fn format_network_path(template: &str, network: u32) -> String {
    template.replace("{0}", &format!("{network:08X}"))
}

fn build_application_logs_settings(
    default_network: u32,
    path: Option<String>,
    network: Option<u32>,
    max_stack_size: Option<usize>,
    debug: Option<bool>,
    unhandled_exception_policy: Option<String>,
) -> ApplicationLogsSettings {
    let path = path.unwrap_or_else(|| "ApplicationLogs_{0}".to_string());
    let network = network.unwrap_or(default_network);
    let max_stack_size = max_stack_size.unwrap_or(u16::MAX as usize);
    let debug = debug.unwrap_or(false);
    let exception_policy = parse_unhandled_exception_policy(
        unhandled_exception_policy.as_deref(),
        UnhandledExceptionPolicy::Ignore,
    );
    ApplicationLogsSettings::new(true, network, path, max_stack_size, debug, exception_policy)
}

fn build_state_service_settings(
    default_network: u32,
    path: Option<String>,
    full_state: Option<bool>,
    network: Option<u32>,
    auto_verify: Option<bool>,
    max_find_result_items: Option<usize>,
    unhandled_exception_policy: Option<String>,
) -> StateServiceSettings {
    let path = path.unwrap_or_else(|| "Data_MPT_{0}".to_string());
    let network = network.unwrap_or(default_network);
    let full_state = full_state.unwrap_or(false);
    let auto_verify = auto_verify.unwrap_or(false);
    let max_find_result_items = max_find_result_items.unwrap_or(100);
    let exception_policy = parse_unhandled_exception_policy(
        unhandled_exception_policy.as_deref(),
        UnhandledExceptionPolicy::StopPlugin,
    );
    StateServiceSettings {
        full_state,
        path,
        network,
        auto_verify,
        max_find_result_items,
        exception_policy,
    }
}

fn build_tokens_tracker_settings(
    default_network: u32,
    db_path: Option<String>,
    track_history: Option<bool>,
    max_results: Option<u32>,
    network: Option<u32>,
    enabled_trackers: Vec<String>,
    unhandled_exception_policy: Option<String>,
) -> TokensTrackerSettings {
    let mut settings = TokensTrackerSettings::default();
    settings.db_path = db_path.unwrap_or_else(|| settings.db_path.clone());
    settings.track_history = track_history.unwrap_or(settings.track_history);
    settings.max_results = max_results.unwrap_or(settings.max_results);
    settings.network = network.unwrap_or(default_network);
    if !enabled_trackers.is_empty() {
        settings.enabled_trackers = enabled_trackers;
    }
    if let Some(policy) = unhandled_exception_policy {
        settings.exception_policy =
            parse_unhandled_exception_policy(Some(policy.as_str()), settings.exception_policy);
    }
    settings
}

#[allow(clippy::too_many_arguments)]
fn build_dbft_settings(
    default_network: u32,
    recovery_logs: Option<String>,
    ignore_recovery_logs: Option<bool>,
    auto_start: Option<bool>,
    network: Option<u32>,
    max_block_size: Option<u32>,
    max_block_system_fee: Option<i64>,
    unhandled_exception_policy: Option<String>,
) -> DbftSettings {
    let mut settings = DbftSettings {
        recovery_logs: "ConsensusState".to_string(),
        ignore_recovery_logs: false,
        auto_start: false,
        network: default_network,
        max_block_size: MAX_BLOCK_SIZE as u32,
        max_block_system_fee: 150_000_000_000,
        exception_policy: UnhandledExceptionPolicy::StopNode,
    };

    if let Some(value) = recovery_logs {
        settings.recovery_logs = value;
    }
    if let Some(ignore) = ignore_recovery_logs {
        settings.ignore_recovery_logs = ignore;
    }
    if let Some(auto_start) = auto_start {
        settings.auto_start = auto_start;
    }
    settings.network = network.unwrap_or(default_network);
    if let Some(size) = max_block_size {
        settings.max_block_size = size;
    }
    if let Some(fee) = max_block_system_fee {
        settings.max_block_system_fee = fee;
    }
    if let Some(policy) = unhandled_exception_policy {
        settings.exception_policy =
            parse_unhandled_exception_policy(Some(policy.as_str()), settings.exception_policy);
    }

    settings
}

#[allow(clippy::too_many_arguments, clippy::field_reassign_with_default)]
fn build_oracle_service_settings(
    default_network: u32,
    network: Option<u32>,
    nodes: Vec<String>,
    max_task_timeout: Option<u64>,
    max_oracle_timeout: Option<u64>,
    allow_private_host: Option<bool>,
    allowed_content_types: Vec<String>,
    https: Option<super::OracleServiceHttpsSection>,
    neofs: Option<OracleServiceNeoFsSection>,
    auto_start: Option<bool>,
    unhandled_exception_policy: Option<String>,
) -> OracleServiceSettings {
    let mut settings = OracleServiceSettings::default();
    settings.network = network.unwrap_or(default_network);
    if !nodes.is_empty() {
        settings.nodes = nodes;
    }
    if let Some(timeout) = max_task_timeout {
        settings.max_task_timeout = Duration::from_millis(timeout);
    }
    if let Some(timeout) = max_oracle_timeout {
        settings.max_oracle_timeout = Duration::from_millis(timeout);
    }
    if let Some(allow) = allow_private_host {
        settings.allow_private_host = allow;
    }
    if !allowed_content_types.is_empty() {
        settings.allowed_content_types = allowed_content_types;
    }
    if let Some(https) = https {
        if let Some(timeout) = https.timeout {
            settings.https_timeout = Duration::from_millis(timeout);
        }
    }
    if let Some(neofs) = neofs {
        apply_neofs_settings(&mut settings, neofs);
    }
    if let Some(auto_start) = auto_start {
        settings.auto_start = auto_start;
    }
    if let Some(policy) = unhandled_exception_policy {
        settings.exception_policy =
            parse_unhandled_exception_policy(Some(policy.as_str()), settings.exception_policy);
    }
    settings.normalize();
    settings
}

fn apply_neofs_settings(settings: &mut OracleServiceSettings, neofs: OracleServiceNeoFsSection) {
    if let Some(endpoint) = neofs.endpoint {
        settings.neofs_endpoint = endpoint;
    }
    if let Some(timeout) = neofs.timeout {
        settings.neofs_timeout = Duration::from_millis(timeout);
    }
    if let Some(token) = neofs.bearer_token {
        settings.neofs_bearer_token = Some(token);
    }
    if let Some(signature) = neofs.bearer_signature {
        settings.neofs_bearer_signature = Some(signature);
    }
    if let Some(key) = neofs.bearer_signature_key {
        settings.neofs_bearer_signature_key = Some(key);
    }
    if let Some(wallet_connect) = neofs.wallet_connect {
        settings.neofs_wallet_connect = wallet_connect;
    }
    if let Some(auto_sign) = neofs.auto_sign_bearer {
        settings.neofs_auto_sign_bearer = auto_sign;
    }
    if let Some(use_grpc) = neofs.use_grpc {
        settings.neofs_use_grpc = use_grpc;
    }
}

fn parse_unhandled_exception_policy(
    value: Option<&str>,
    default_policy: UnhandledExceptionPolicy,
) -> UnhandledExceptionPolicy {
    let Some(raw) = value else {
        return default_policy;
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "ignore" => UnhandledExceptionPolicy::Ignore,
        "stopplugin" => UnhandledExceptionPolicy::StopPlugin,
        "continue" => UnhandledExceptionPolicy::Continue,
        "terminate" => UnhandledExceptionPolicy::Terminate,
        "stopnode" => UnhandledExceptionPolicy::StopNode,
        _ => default_policy,
    }
}

pub(super) fn validate_oracle_nodes(nodes: &[String]) -> Result<()> {
    for node in nodes {
        if let Err(err) = Url::parse(node) {
            bail!(
                "OracleService.Nodes entry '{}' is not a valid absolute URI: {}",
                node,
                err
            );
        }
    }
    Ok(())
}
