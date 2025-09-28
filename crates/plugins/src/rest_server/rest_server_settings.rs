// Copyright (C) 2015-2025 The Neo Project.
//
// rest_server_settings.rs belongs to the Neo project and is licensed
// under the MIT license. See the LICENSE file in the project root
// for more information.

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;

/// Compression level supported by the REST server (maps to
/// `System.IO.Compression.CompressionLevel`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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

/// Contract resolver used when serialising JSON payloads.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContractResolver {
    CamelCasePropertyNames,
}

impl Default for ContractResolver {
    fn default() -> Self {
        ContractResolver::CamelCasePropertyNames
    }
}

/// Behaviour when encountering missing members while deserialising.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MissingMemberHandling {
    Ignore,
    Error,
}

impl Default for MissingMemberHandling {
    fn default() -> Self {
        MissingMemberHandling::Error
    }
}

/// Behaviour for serialising `null` values.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NullValueHandling {
    Include,
    Ignore,
}

impl Default for NullValueHandling {
    fn default() -> Self {
        NullValueHandling::Include
    }
}

/// Formatting applied to JSON output.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum JsonFormatting {
    None,
    Indented,
}

impl Default for JsonFormatting {
    fn default() -> Self {
        JsonFormatting::None
    }
}

/// Known converters that correspond to the Newtonsoft converters
/// registered by the C# plugin. Each variant maps one-to-one to the
/// converter type used in the original implementation.
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    ECPointJsonConverter,
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

impl JsonConverterKind {
    fn all() -> Vec<JsonConverterKind> {
        use JsonConverterKind::*;
        vec![
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
            ECPointJsonConverter,
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
        ]
    }
}

/// Serialiser settings used by the REST server when encoding and
/// decoding JSON payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonSerializerSettings {
    pub contract_resolver: ContractResolver,
    pub missing_member_handling: MissingMemberHandling,
    pub null_value_handling: NullValueHandling,
    pub formatting: JsonFormatting,
    pub converters: Vec<JsonConverterKind>,
}

impl Default for JsonSerializerSettings {
    fn default() -> Self {
        Self {
            contract_resolver: ContractResolver::default(),
            missing_member_handling: MissingMemberHandling::default(),
            null_value_handling: NullValueHandling::default(),
            formatting: JsonFormatting::default(),
            converters: JsonConverterKind::all(),
        }
    }
}

/// Full set of configuration options used by the REST server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestServerSettings {
    pub network: u32,
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
            port: 10339,
            keep_alive_timeout: 120,
            ssl_cert_file: None,
            ssl_cert_password: None,
            trusted_authorities: Vec::new(),
            enable_basic_authentication: false,
            rest_user: String::new(),
            rest_pass: String::new(),
            enable_cors: false,
            allow_origins: Vec::new(),
            disable_controllers: Vec::new(),
            enable_compression: false,
            compression_level: CompressionLevel::default(),
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

static CURRENT_SETTINGS: Lazy<RwLock<RestServerSettings>> =
    Lazy::new(|| RwLock::new(RestServerSettings::default()));

impl RestServerSettings {
    /// Load settings from an optional JSON configuration value.
    /// When `config` is `None`, defaults are applied.
    pub fn load(config: Option<&Value>) {
        let settings = config
            .map(RestServerSettings::from_config)
            .unwrap_or_else(RestServerSettings::default);
        *CURRENT_SETTINGS.write() = settings;
    }

    /// Returns the currently active configuration snapshot.
    pub fn current() -> RestServerSettings {
        CURRENT_SETTINGS.read().clone()
    }

    fn from_config(config: &Value) -> Self {
        let mut result = RestServerSettings::default();

        if let Some(network) = config.get("Network").and_then(Value::as_u64) {
            result.network = network as u32;
        }

        if let Some(address) = config
            .get("BindAddress")
            .and_then(Value::as_str)
            .and_then(|s| IpAddr::from_str(s).ok())
            .or_else(|| Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED)))
        {
            result.bind_address = address;
        }

        if let Some(port) = config.get("Port").and_then(Value::as_u64) {
            result.port = port as u16;
        }

        if let Some(keep_alive) = config.get("KeepAliveTimeout").and_then(Value::as_u64) {
            result.keep_alive_timeout = keep_alive as u32;
        }

        result.ssl_cert_file = config
            .get("SslCertFile")
            .and_then(Value::as_str)
            .map(String::from);
        result.ssl_cert_password = config
            .get("SslCertPassword")
            .and_then(Value::as_str)
            .map(String::from);

        if let Some(authorities) = config.get("TrustedAuthorities").and_then(Value::as_array) {
            result.trusted_authorities = authorities
                .iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect();
        }

        if let Some(flag) = config
            .get("EnableBasicAuthentication")
            .and_then(Value::as_bool)
        {
            result.enable_basic_authentication = flag;
        }

        if let Some(user) = config.get("RestUser").and_then(Value::as_str) {
            result.rest_user = user.to_string();
        }

        if let Some(pass) = config.get("RestPass").and_then(Value::as_str) {
            result.rest_pass = pass.to_string();
        }

        if let Some(flag) = config.get("EnableCors").and_then(Value::as_bool) {
            result.enable_cors = flag;
        }

        if let Some(origins) = config.get("AllowOrigins").and_then(Value::as_array) {
            result.allow_origins = origins
                .iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect();
        }

        if let Some(disabled) = config.get("DisableControllers").and_then(Value::as_array) {
            result.disable_controllers = disabled
                .iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect();
        }

        if let Some(flag) = config.get("EnableCompression").and_then(Value::as_bool) {
            result.enable_compression = flag;
        }

        if let Some(level) = config.get("CompressionLevel").and_then(Value::as_str) {
            if let Ok(parsed) = CompressionLevel::from_str(level) {
                result.compression_level = parsed;
            }
        }

        if let Some(flag) = config
            .get("EnableForwardedHeaders")
            .and_then(Value::as_bool)
        {
            result.enable_forwarded_headers = flag;
        }

        if let Some(flag) = config.get("EnableSwagger").and_then(Value::as_bool) {
            result.enable_swagger = flag;
        }

        if let Some(page_size) = config.get("MaxPageSize").and_then(Value::as_u64) {
            result.max_page_size = page_size as u32;
        }

        if let Some(connections) = config
            .get("MaxConcurrentConnections")
            .and_then(Value::as_i64)
        {
            result.max_concurrent_connections = connections;
        }

        if let Some(max_gas) = config.get("MaxGasInvoke").and_then(Value::as_i64) {
            result.max_gas_invoke = max_gas;
        }

        if let Some(flag) = config.get("EnableRateLimiting").and_then(Value::as_bool) {
            result.enable_rate_limiting = flag;
        }

        if let Some(limit) = config.get("RateLimitPermitLimit").and_then(Value::as_i64) {
            result.rate_limit_permit_limit = limit as i32;
        }

        if let Some(window) = config.get("RateLimitWindowSeconds").and_then(Value::as_i64) {
            result.rate_limit_window_seconds = window as i32;
        }

        if let Some(queue) = config.get("RateLimitQueueLimit").and_then(Value::as_i64) {
            result.rate_limit_queue_limit = queue as i32;
        }

        result
    }
}

impl FromStr for CompressionLevel {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "Optimal" => Ok(CompressionLevel::Optimal),
            "Fastest" => Ok(CompressionLevel::Fastest),
            "NoCompression" => Ok(CompressionLevel::NoCompression),
            "SmallestSize" => Ok(CompressionLevel::SmallestSize),
            _ => Err(()),
        }
    }
}
