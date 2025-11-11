use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

pub(super) fn extract_json_protocol(value: JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(mut map) => map
            .remove("ProtocolConfiguration")
            .unwrap_or(JsonValue::Object(map)),
        other => other,
    }
}

pub(super) fn extract_toml_protocol(value: TomlValue) -> TomlValue {
    match value {
        TomlValue::Table(mut table) => table
            .remove("ProtocolConfiguration")
            .unwrap_or(TomlValue::Table(table)),
        other => other,
    }
}
