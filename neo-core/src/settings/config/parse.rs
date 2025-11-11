use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

use crate::settings::error::ProtocolSettingsError;

use super::{model::ProtocolSettingsConfig, section};

impl ProtocolSettingsConfig {
    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, ProtocolSettingsError> {
        let value: JsonValue = serde_json::from_slice(bytes)
            .map_err(|err| ProtocolSettingsError::JsonError(err.to_string()))?;
        Self::from_json_value(value)
    }

    pub fn from_json_str(input: &str) -> Result<Self, ProtocolSettingsError> {
        Self::from_json_slice(input.as_bytes())
    }

    pub fn from_toml_str(input: &str) -> Result<Self, ProtocolSettingsError> {
        let value: TomlValue = toml::from_str(input)
            .map_err(|err| ProtocolSettingsError::TomlError(err.to_string()))?;
        Self::from_toml_value(value)
    }

    fn from_json_value(value: JsonValue) -> Result<Self, ProtocolSettingsError> {
        let section = section::extract_json_protocol(value);
        serde_json::from_value(section)
            .map_err(|err| ProtocolSettingsError::JsonError(err.to_string()))
    }

    fn from_toml_value(value: TomlValue) -> Result<Self, ProtocolSettingsError> {
        let section = section::extract_toml_protocol(value);
        section
            .try_into()
            .map_err(|err: toml::de::Error| ProtocolSettingsError::TomlError(err.to_string()))
    }
}
