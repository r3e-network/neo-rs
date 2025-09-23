//! Port of `RestServerSettings.cs`.
//!
//! The structure mirrors the C# configuration contract so that the Rust REST
//! server can be configured with the same JSON file (`RestServer.json`).

use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    net::{IpAddr, Ipv4Addr},
    path::Path,
    sync::RwLock,
};

/// Global storage for the currently loaded settings.
static CURRENT: Lazy<RwLock<RestServerSettings>> =
    Lazy::new(|| RwLock::new(RestServerSettings::default()));

/// Compression level options used when HTTP compression is enabled.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum CompressionLevel {
    Optimal,
    Fastest,
    NoCompression,
    SmallestSize,
}

impl Default for CompressionLevel {
    fn default() -> Self {
        CompressionLevel::SmallestSize
    }
}

/// JSON contract resolver used by the REST server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContractResolverKind {
    #[serde(rename = "CamelCasePropertyNamesContractResolver")]
    CamelCasePropertyNames,
}

impl Default for ContractResolverKind {
    fn default() -> Self {
        ContractResolverKind::CamelCasePropertyNames
    }
}

/// Behaviour when JSON payloads are missing members.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum MissingMemberHandling {
    Error,
    Ignore,
}

impl Default for MissingMemberHandling {
    fn default() -> Self {
        MissingMemberHandling::Error
    }
}

/// Behaviour when encountering null values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum NullValueHandling {
    Include,
    Ignore,
}

impl Default for NullValueHandling {
    fn default() -> Self {
        NullValueHandling::Include
    }
}

/// JSON formatting option.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum Formatting {
    None,
    Indented,
}

impl Default for Formatting {
    fn default() -> Self {
        Formatting::None
    }
}

/// Enumeration of all JSON converter types required for feature parity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum JsonConverterKind {
    StringEnumConverter,
    BigDecimalJsonConverter,
    BlockHeaderJsonConverter,
    BlockJsonConverter,
    ContractAbiJsonConverter,
    ContractEventDescriptorJsonConverter,
    ContractGroupJsonConverter,
    ContractInvokeParametersJsonConverter,
    ContractJsonConverter,
    ContractManifestJsonConverter,
    ContractMethodJsonConverter,
    ContractMethodParametersJsonConverter,
    ContractParameterDefinitionJsonConverter,
    ContractParameterJsonConverter,
    ContractPermissionDescriptorJsonConverter,
    ContractPermissionJsonConverter,
    EcPointJsonConverter,
    GuidJsonConverter,
    InteropInterfaceJsonConverter,
    MethodTokenJsonConverter,
    NefFileJsonConverter,
    ReadOnlyMemoryBytesJsonConverter,
    SignerJsonConverter,
    StackItemJsonConverter,
    TransactionAttributeJsonConverter,
    TransactionJsonConverter,
    UInt160JsonConverter,
    UInt256JsonConverter,
    VmArrayJsonConverter,
    VmBooleanJsonConverter,
    VmBufferJsonConverter,
    VmByteStringJsonConverter,
    VmIntegerJsonConverter,
    VmMapJsonConverter,
    VmNullJsonConverter,
    VmPointerJsonConverter,
    VmStructJsonConverter,
    WitnessConditionJsonConverter,
    WitnessJsonConverter,
    WitnessRuleJsonConverter,
}

/// Sub-structure that mirrors the Newtonsoft Json serializer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct JsonSerializerSettings {
    pub contract_resolver: ContractResolverKind,
    pub missing_member_handling: MissingMemberHandling,
    pub null_value_handling: NullValueHandling,
    pub formatting: Formatting,
    pub converters: Vec<JsonConverterKind>,
}

impl Default for JsonSerializerSettings {
    fn default() -> Self {
        Self {
            contract_resolver: ContractResolverKind::default(),
            missing_member_handling: MissingMemberHandling::default(),
            null_value_handling: NullValueHandling::default(),
            formatting: Formatting::default(),
            converters: vec![
                JsonConverterKind::StringEnumConverter,
                JsonConverterKind::BigDecimalJsonConverter,
                JsonConverterKind::BlockHeaderJsonConverter,
                JsonConverterKind::BlockJsonConverter,
                JsonConverterKind::ContractAbiJsonConverter,
                JsonConverterKind::ContractEventDescriptorJsonConverter,
                JsonConverterKind::ContractGroupJsonConverter,
                JsonConverterKind::ContractInvokeParametersJsonConverter,
                JsonConverterKind::ContractJsonConverter,
                JsonConverterKind::ContractManifestJsonConverter,
                JsonConverterKind::ContractMethodJsonConverter,
                JsonConverterKind::ContractMethodParametersJsonConverter,
                JsonConverterKind::ContractParameterDefinitionJsonConverter,
                JsonConverterKind::ContractParameterJsonConverter,
                JsonConverterKind::ContractPermissionDescriptorJsonConverter,
                JsonConverterKind::ContractPermissionJsonConverter,
                JsonConverterKind::EcPointJsonConverter,
                JsonConverterKind::GuidJsonConverter,
                JsonConverterKind::InteropInterfaceJsonConverter,
                JsonConverterKind::MethodTokenJsonConverter,
                JsonConverterKind::NefFileJsonConverter,
                JsonConverterKind::ReadOnlyMemoryBytesJsonConverter,
                JsonConverterKind::SignerJsonConverter,
                JsonConverterKind::StackItemJsonConverter,
                JsonConverterKind::TransactionAttributeJsonConverter,
                JsonConverterKind::TransactionJsonConverter,
                JsonConverterKind::UInt160JsonConverter,
                JsonConverterKind::UInt256JsonConverter,
                JsonConverterKind::VmArrayJsonConverter,
                JsonConverterKind::VmBooleanJsonConverter,
                JsonConverterKind::VmBufferJsonConverter,
                JsonConverterKind::VmByteStringJsonConverter,
                JsonConverterKind::VmIntegerJsonConverter,
                JsonConverterKind::VmMapJsonConverter,
                JsonConverterKind::VmNullJsonConverter,
                JsonConverterKind::VmPointerJsonConverter,
                JsonConverterKind::VmStructJsonConverter,
                JsonConverterKind::WitnessConditionJsonConverter,
                JsonConverterKind::WitnessJsonConverter,
                JsonConverterKind::WitnessRuleJsonConverter,
            ],
        }
    }
}

/// Rust representation of `RestServerSettings`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RestServerSettings {
    pub network: u32,
    #[serde(with = "serde_ip_addr")]
    pub bind_address: IpAddr,
    pub port: u16,
    pub keep_alive_timeout: u32,
    pub ssl_cert_file: Option<String>,
    pub ssl_cert_password: Option<String>,
    pub trusted_authorities: Vec<String>,
    pub enable_basic_authentication: bool,
    pub rest_user: String,
    pub rest_pass: String,
    pub enable_cors: bool,
    pub allow_origins: Vec<String>,
    pub disable_controllers: Vec<String>,
    pub enable_compression: bool,
    pub compression_level: CompressionLevel,
    pub enable_forwarded_headers: bool,
    pub enable_swagger: bool,
    pub max_page_size: u32,
    pub max_concurrent_connections: i64,
    pub max_gas_invoke: i64,
    pub enable_rate_limiting: bool,
    pub rate_limit_permit_limit: i32,
    pub rate_limit_window_seconds: i32,
    pub rate_limit_queue_limit: i32,
    pub json_serializer_settings: JsonSerializerSettings,
}

impl Default for RestServerSettings {
    fn default() -> Self {
        Self {
            network: 860_833_102,
            bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 10_339,
            keep_alive_timeout: 120,
            ssl_cert_file: Some(String::new()),
            ssl_cert_password: Some(String::new()),
            trusted_authorities: Vec::new(),
            enable_basic_authentication: false,
            rest_user: String::new(),
            rest_pass: String::new(),
            enable_cors: false,
            allow_origins: Vec::new(),
            disable_controllers: Vec::new(),
            enable_compression: false,
            compression_level: CompressionLevel::SmallestSize,
            enable_forwarded_headers: false,
            enable_swagger: false,
            max_page_size: 50,
            max_concurrent_connections: 40,
            max_gas_invoke: 200_000_000,
            enable_rate_limiting: false,
            rate_limit_permit_limit: 10,
            rate_limit_window_seconds: 60,
            rate_limit_queue_limit: 0,
            json_serializer_settings: JsonSerializerSettings::default(),
        }
    }
}

impl RestServerSettings {
    /// Load settings from the given JSON path and store them as the current instance.
    pub fn load_from_path(path: &Path) -> Result<RestServerSettings> {
        let settings = if path.exists() {
            let raw = fs::read_to_string(path).with_context(|| {
                format!("failed to read REST server config: {}", path.display())
            })?;
            if raw.trim().is_empty() {
                RestServerSettings::default()
            } else {
                serde_json::from_str::<RestServerSettings>(&raw).with_context(|| {
                    format!("failed to parse REST server config: {}", path.display())
                })?
            }
        } else {
            RestServerSettings::default()
        };

        if let Ok(mut guard) = CURRENT.write() {
            *guard = settings.clone();
        }

        Ok(settings)
    }

    /// Replace the current settings (useful for tests).
    pub fn set_current(settings: RestServerSettings) {
        if let Ok(mut guard) = CURRENT.write() {
            *guard = settings;
        }
    }

    /// Retrieve the currently loaded settings (cloned).
    pub fn current() -> RestServerSettings {
        CURRENT.read().cloned().unwrap_or_default()
    }
}

mod serde_ip_addr {
    use serde::{de::Error, Deserialize, Deserializer, Serializer};
    use std::net::{IpAddr, Ipv4Addr};

    pub fn serialize<S>(addr: &IpAddr, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&addr.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<IpAddr, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Option::<String>::deserialize(deserializer)?;
        match value {
            Some(s) if !s.is_empty() => s
                .parse::<IpAddr>()
                .map_err(|err| D::Error::custom(format!("invalid IP address `{}`: {}", s, err))),
            _ => Ok(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
        }
    }
}
