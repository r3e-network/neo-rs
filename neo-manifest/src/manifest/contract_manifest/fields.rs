//! Typed JSON-adjacent fields owned by `ContractManifest`.
//!
//! Neo manifests expose `features` and `extra` as JSON fields on the wire. The
//! rest of the code should not need raw `serde_json::Value` plumbing to express
//! their invariants: `features` must stay empty for Neo N3, and `extra` is
//! either absent/null or a JSON object.

use std::collections::HashMap;

use neo_error::{CoreError, CoreResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Neo N3 manifest `features` field.
///
/// C# currently requires this map to be empty. Keeping a named type makes that
/// invariant explicit while preserving the JSON/wire representation as `{}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct ManifestFeatures(HashMap<String, Value>);

impl ManifestFeatures {
    /// Returns an empty features map.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Returns whether the feature map has no entries.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Clears all feature entries.
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Inserts a feature entry.
    ///
    /// This is primarily useful for tests and validation of malformed
    /// constructed manifests; deployable Neo N3 manifests must still validate
    /// with an empty map.
    pub fn insert(&mut self, key: String, value: Value) -> Option<Value> {
        self.0.insert(key, value)
    }
}

impl From<HashMap<String, Value>> for ManifestFeatures {
    fn from(features: HashMap<String, Value>) -> Self {
        Self(features)
    }
}

/// Neo N3 manifest `extra` field when present.
///
/// `None` on `ContractManifest::extra` represents absent/null. A present value
/// must be a JSON object, matching C# manifest parsing and stack projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestExtra(Value);

impl ManifestExtra {
    /// Creates a manifest extra object from a JSON value.
    pub fn from_value(value: Value) -> CoreResult<Self> {
        match value {
            Value::Object(_) => Ok(Self(value)),
            Value::Null => Err(CoreError::invalid_data(
                "Manifest extra null is represented by None",
            )),
            _ => Err(CoreError::invalid_data(
                "ContractManifest extra must be a JSON object",
            )),
        }
    }

    /// Borrows the JSON object value for C#-compatible encoding.
    pub fn as_value(&self) -> &Value {
        &self.0
    }
}

impl Serialize for ManifestExtra {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ManifestExtra {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        ManifestExtra::from_value(value).map_err(serde::de::Error::custom)
    }
}
