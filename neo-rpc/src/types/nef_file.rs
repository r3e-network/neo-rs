use super::RpcMethodToken;
use super::json::{object_array, parse_object_array_lossy};
use base64::{Engine as _, engine::general_purpose};
use neo_error::{CoreError, CoreResult};
use neo_manifest::NefFile;
use neo_serialization::json::{JObject, JToken};

/// RPC NEF file helper matching C# `RpcNefFile`
pub struct RpcNefFile {
    /// The NEF file
    pub nef_file: NefFile,
}

impl RpcNefFile {
    /// Creates a new wrapper from a NEF file
    #[must_use]
    pub const fn new(nef_file: NefFile) -> Self {
        Self { nef_file }
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let compiler = json
            .get("compiler")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'compiler' field"))?;

        let source = json
            .get("source")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'source' field"))?;

        let tokens = parse_object_array_lossy(json, "tokens", RpcMethodToken::from_json);

        let script = json
            .get("script")
            .and_then(neo_serialization::json::JToken::as_string)
            .and_then(|s| general_purpose::STANDARD.decode(s).ok())
            .ok_or_else(|| CoreError::other("Missing or invalid 'script' field"))?;

        let checksum = json
            .get("checksum")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'checksum' field"))?
            as u32;

        Ok(Self {
            nef_file: NefFile {
                compiler,
                source,
                tokens: tokens.into_iter().map(|t| t.method_token).collect(),
                script,
                checksum,
            },
        })
    }

    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "magic".to_string(),
            JToken::Number(f64::from(NefFile::MAGIC)),
        );
        json.insert(
            "compiler".to_string(),
            JToken::String(self.nef_file.compiler.clone()),
        );
        json.insert(
            "source".to_string(),
            JToken::String(self.nef_file.source.clone()),
        );
        json.insert(
            "tokens".to_string(),
            object_array(&self.nef_file.tokens, |t| {
                RpcMethodToken {
                    method_token: t.clone(),
                }
                .to_json()
            }),
        );
        json.insert(
            "script".to_string(),
            JToken::String(general_purpose::STANDARD.encode(&self.nef_file.script)),
        );
        json.insert(
            "checksum".to_string(),
            JToken::Number(f64::from(self.nef_file.checksum)),
        );
        json
    }
}

#[cfg(test)]
#[path = "../tests/types/nef_file.rs"]
mod tests;
