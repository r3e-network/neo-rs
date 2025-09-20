//!
//! This module provides the WildcardContainer type which can either hold
//! specific values or represent a wildcard (all values).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// Container that represents either a wildcard (matches all values) or an explicit list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WildcardContainer<T> {
    /// Matches any value.
    Wildcard,
    /// Explicit list of allowed values.
    List(Vec<T>),
}

impl<T> WildcardContainer<T> {
    /// Creates a container holding the provided values.
    pub fn create(values: Vec<T>) -> Self {
        Self::List(values)
    }

    /// Creates a wildcard container that matches anything.
    pub fn create_wildcard() -> Self {
        Self::Wildcard
    }

    /// Returns true when the container is a wildcard.
    pub fn is_wildcard(&self) -> bool {
        matches!(self, Self::Wildcard)
    }

    /// Number of explicit values stored (0 for wildcard).
    pub fn count(&self) -> usize {
        match self {
            Self::Wildcard => 0,
            Self::List(values) => values.len(),
        }
    }

    /// Alias for compatibility with previous APIs.
    pub fn len(&self) -> usize {
        self.count()
    }

    /// Returns true if there are no explicit values.
    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    /// Returns a reference to the underlying values when present.
    pub fn values(&self) -> Option<&[T]> {
        match self {
            Self::Wildcard => None,
            Self::List(values) => Some(values.as_slice()),
        }
    }

    /// Checks whether the provided value is contained or matched by the wildcard.
    pub fn contains(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        match self {
            Self::Wildcard => true,
            Self::List(values) => values.iter().any(|item| item == value),
        }
    }
}

impl<T> Default for WildcardContainer<T> {
    fn default() -> Self {
        Self::create_wildcard()
    }
}

impl<T: Serialize> Serialize for WildcardContainer<T> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Wildcard => serializer.serialize_str("*"),
            Self::List(values) => values.serialize(serializer),
        }
    }
}

impl<'de, T> Deserialize<'de> for WildcardContainer<T>
where
    T: DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;

        match value {
            Value::String(s) if s == "*" => Ok(Self::Wildcard),
            Value::Array(_) => {
                let values: Vec<T> = serde_json::from_value(value)
                    .map_err(|err| <D::Error as serde::de::Error>::custom(err.to_string()))?;
                Ok(Self::List(values))
            }
            other => Err(serde::de::Error::custom(format!(
                "Expected '*' or array for WildcardContainer, found {}",
                other
            ))),
        }
    }
}
