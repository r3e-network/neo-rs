use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// This file contains types and helper methods for wildcard containers.
// A wildcard container can contain either a finite set of elements or
// every possible element, in which case it is named `wildcard`.

/// Represents a string set which can be a wildcard.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WildStrings {
    Wildcard,
    Set(HashSet<String>),
}

/// Represents a PermissionDescriptor set which can be a wildcard.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WildPermissionDescs {
    Wildcard,
    Set(HashSet<PermissionDesc>),
}

impl WildStrings {
    /// Checks if v is in the container.
    pub fn contains(&self, v: &str) -> bool {
        match self {
            WildStrings::Wildcard => true,
            WildStrings::Set(set) => set.contains(v),
        }
    }

    /// Returns true iff the container is a wildcard.
    pub fn is_wildcard(&self) -> bool {
        matches!(self, WildStrings::Wildcard)
    }

    /// Transforms the container into an empty one.
    pub fn restrict(&mut self) {
        *self = WildStrings::Set(HashSet::new());
    }

    /// Adds v to the container.
    pub fn add(&mut self, v: String) {
        match self {
            WildStrings::Wildcard => {}
            WildStrings::Set(set) => {
                set.insert(v);
            }
        }
    }
}

impl WildPermissionDescs {
    /// Checks if v is in the container.
    pub fn contains(&self, v: &PermissionDesc) -> bool {
        match self {
            WildPermissionDescs::Wildcard => true,
            WildPermissionDescs::Set(set) => set.contains(v),
        }
    }

    /// Returns true iff the container is a wildcard.
    pub fn is_wildcard(&self) -> bool {
        matches!(self, WildPermissionDescs::Wildcard)
    }

    /// Transforms the container into an empty one.
    pub fn restrict(&mut self) {
        *self = WildPermissionDescs::Set(HashSet::new());
    }

    /// Adds v to the container and converts container to non-wildcard (if it's still wildcard).
    pub fn add(&mut self, v: PermissionDesc) {
        match self {
            WildPermissionDescs::Wildcard => {
                let mut set = HashSet::new();
                set.insert(v);
                *self = WildPermissionDescs::Set(set);
            }
            WildPermissionDescs::Set(set) => {
                set.insert(v);
            }
        }
    }
}

impl Serialize for WildStrings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            WildStrings::Wildcard => serializer.serialize_str("*"),
            WildStrings::Set(set) => set.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for WildStrings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value: serde_json::Value = Deserialize::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(s) if s == "*" => Ok(WildStrings::Wildcard),
            _ => {
                let set: HashSet<String> = Deserialize::deserialize(value)?;
                Ok(WildStrings::Set(set))
            }
        }
    }
}

impl Serialize for WildPermissionDescs {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            WildPermissionDescs::Wildcard => serializer.serialize_str("*"),
            WildPermissionDescs::Set(set) => set.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for WildPermissionDescs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value: serde_json::Value = Deserialize::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(s) if s == "*" => Ok(WildPermissionDescs::Wildcard),
            _ => {
                let set: HashSet<PermissionDesc> = Deserialize::deserialize(value)?;
                Ok(WildPermissionDescs::Set(set))
            }
        }
    }
}
