use super::super::utility::{object_array, parse_object_array_lossy};
use super::RpcMethodToken;
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

        let tokens = parse_object_array_lossy(json, "tokens", |obj| {
            RpcMethodToken::from_json(obj).map_err(|e| e.to_string())
        });

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
mod tests {
    use super::super::test_fixtures::rpc_case_result;
    use super::*;
    use neo_manifest::MethodToken;

    fn sample_nef() -> NefFile {
        NefFile {
            compiler: "neo".into(),
            source: "src".into(),
            tokens: vec![MethodToken::default()],
            script: vec![1, 2, 3],
            checksum: 999,
        }
    }

    #[test]
    fn rpc_nef_file_roundtrip() {
        let nef = sample_nef();
        let rpc = RpcNefFile::new(nef.clone());
        let json = rpc.to_json();
        let parsed = RpcNefFile::from_json(&json).expect("nef");
        assert_eq!(parsed.nef_file.compiler, nef.compiler);
        assert_eq!(parsed.nef_file.tokens.len(), nef.tokens.len());
        assert_eq!(parsed.nef_file.script, nef.script);
        assert_eq!(parsed.nef_file.checksum, nef.checksum);
    }

    #[test]
    fn rpc_nef_file_rejects_missing_script() {
        let mut json = JObject::new();
        json.insert("compiler".to_string(), JToken::String("neo".into()));
        json.insert("source".to_string(), JToken::String("src".into()));
        json.insert(
            "tokens".to_string(),
            JToken::Array(neo_serialization::json::JArray::new()),
        );
        json.insert("checksum".to_string(), JToken::Number(1f64));

        assert!(RpcNefFile::from_json(&json).is_err());
    }

    #[test]
    fn nef_to_json_matches_rpc_test_case() {
        let Some(result) = rpc_case_result("getcontractstateasync") else {
            return;
        };
        let expected = result
            .get("nef")
            .and_then(JToken::as_object)
            .expect("nef result");
        let parsed = RpcNefFile::from_json(expected).expect("parse");
        let actual = parsed.to_json();
        assert_eq!(expected.to_string(), actual.to_string());
    }
}
