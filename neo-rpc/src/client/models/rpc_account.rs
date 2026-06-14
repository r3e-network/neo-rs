use super::super::utility::insert_optional_string;
use neo_error::{CoreError, CoreResult};
use neo_serialization::json::{JObject, JToken};
use serde::{Deserialize, Serialize};

/// Account information matching C# `RpcAccount`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcAccount {
    /// Account address
    pub address: String,

    /// Whether the account has a key
    pub has_key: bool,

    /// Account label
    pub label: Option<String>,

    /// Whether this is a watch-only account
    pub watch_only: bool,
}

impl RpcAccount {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("address".to_string(), JToken::String(self.address.clone()));
        json.insert("haskey".to_string(), JToken::Boolean(self.has_key));
        insert_optional_string(&mut json, "label", self.label.as_deref());
        json.insert("watchonly".to_string(), JToken::Boolean(self.watch_only));
        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let address = json
            .get("address")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'address' field"))?;

        let has_key = json
            .get("haskey")
            .map(neo_serialization::json::JToken::as_boolean)
            .ok_or_else(|| CoreError::other("Missing or invalid 'haskey' field"))?;

        let label = json
            .get("label")
            .and_then(neo_serialization::json::JToken::as_string);

        let watch_only = json
            .get("watchonly")
            .map(neo_serialization::json::JToken::as_boolean)
            .ok_or_else(|| CoreError::other("Missing or invalid 'watchonly' field"))?;

        Ok(Self {
            address,
            has_key,
            label,
            watch_only,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::rpc_case_result;
    use super::*;

    #[test]
    fn rpc_account_roundtrip_with_label() {
        let account = RpcAccount {
            address: "NQ7cbaBqX1p5quJDQr6b1qnBZBHae3mJzA".to_string(),
            has_key: true,
            label: Some("main".to_string()),
            watch_only: false,
        };

        let json = account.to_json();
        let parsed = RpcAccount::from_json(&json).expect("account");
        assert_eq!(parsed.address, account.address);
        assert_eq!(parsed.has_key, account.has_key);
        assert_eq!(parsed.label, account.label);
        assert_eq!(parsed.watch_only, account.watch_only);
    }

    #[test]
    fn rpc_account_roundtrip_without_label() {
        let account = RpcAccount {
            address: "NQ7cbaBqX1p5quJDQr6b1qnBZBHae3mJzA".to_string(),
            has_key: false,
            label: None,
            watch_only: true,
        };

        let json = account.to_json();
        assert!(matches!(json.get("label"), Some(JToken::Null)));
        let parsed = RpcAccount::from_json(&json).expect("account");
        assert!(parsed.label.is_none());
        assert!(parsed.watch_only);
    }

    #[test]
    fn account_to_json_matches_rpc_test_case() {
        let Some(expected) = rpc_case_result("importprivkeyasync") else {
            return;
        };
        let parsed = RpcAccount::from_json(&expected).expect("parse");
        let actual = parsed.to_json();
        assert_eq!(expected.to_string(), actual.to_string());
    }
}
