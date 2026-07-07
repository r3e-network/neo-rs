//! JSON projection for transaction attributes.

use base64::{Engine as _, engine::general_purpose};

use super::TransactionAttribute;

impl TransactionAttribute {
    /// Converts the attribute to a JSON object.
    /// Matches C# ToJson method.
    pub fn to_json(&self) -> serde_json::Value {
        let mut json = serde_json::json!({
            "type": self.type_id().to_string(),
        });

        match self {
            Self::OracleResponse(attr) => {
                if let Some(obj) = json.as_object_mut() {
                    obj.insert("id".to_string(), serde_json::json!(attr.id));
                    obj.insert("code".to_string(), serde_json::json!(attr.code));
                    obj.insert(
                        "result".to_string(),
                        serde_json::json!(general_purpose::STANDARD.encode(&attr.result)),
                    );
                }
            }
            Self::NotValidBefore(attr) => {
                if let Some(obj) = json.as_object_mut() {
                    obj.insert("height".to_string(), serde_json::json!(attr.height));
                }
            }
            Self::Conflicts(attr) => {
                if let Some(obj) = json.as_object_mut() {
                    obj.insert("hash".to_string(), serde_json::json!(attr.hash.to_string()));
                }
            }
            Self::NotaryAssisted(attr) => {
                if let Some(obj) = json.as_object_mut() {
                    obj.insert("nkeys".to_string(), serde_json::json!(attr.nkeys));
                }
            }
            _ => {}
        }

        json
    }
}
