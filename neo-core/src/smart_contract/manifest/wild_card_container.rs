//! WildCardContainer - matches C# Neo.SmartContract.Manifest.WildCardContainer exactly

use neo_vm::StackItem;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A list that supports wildcard (matches C# WildcardContainer<T>)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WildCardContainer<T> {
    /// Wildcard - allows any value
    #[serde(
        serialize_with = "serialize_wildcard",
        deserialize_with = "deserialize_wildcard"
    )]
    Wildcard,
    /// Specific list of allowed values
    List(Vec<T>),
}

impl<T> WildCardContainer<T> {
    /// Creates a new instance with the initial elements
    pub fn create(data: Vec<T>) -> Self {
        WildCardContainer::List(data)
    }

    /// Creates a new instance with wildcard
    pub fn create_wildcard() -> Self {
        WildCardContainer::Wildcard
    }

    /// Indicates whether the list is a wildcard
    pub fn is_wildcard(&self) -> bool {
        matches!(self, WildCardContainer::Wildcard)
    }

    /// Gets the count of elements
    pub fn count(&self) -> usize {
        match self {
            WildCardContainer::Wildcard => 0,
            WildCardContainer::List(data) => data.len(),
        }
    }

    /// Gets element at index
    pub fn get(&self, index: usize) -> Option<&T> {
        match self {
            WildCardContainer::Wildcard => None,
            WildCardContainer::List(data) => data.get(index),
        }
    }

    /// Converts to JSON representation
    pub fn to_json(&self) -> serde_json::Value
    where
        T: Serialize,
    {
        match self {
            WildCardContainer::Wildcard => serde_json::Value::String("*".to_string()),
            WildCardContainer::List(data) => {
                serde_json::to_value(data).unwrap_or(serde_json::Value::Null)
            }
        }
    }

    /// Creates from JSON representation
    pub fn from_json(json: &serde_json::Value) -> Result<Self, String>
    where
        T: for<'de> Deserialize<'de>,
    {
        match json {
            serde_json::Value::String(s) if s == "*" => Ok(WildCardContainer::Wildcard),
            serde_json::Value::Array(_) => {
                let data: Vec<T> =
                    serde_json::from_value(json.clone()).map_err(|e| e.to_string())?;
                Ok(WildCardContainer::List(data))
            }
            _ => Err("Invalid WildCardContainer format".to_string()),
        }
    }
}

impl<T> Default for WildCardContainer<T> {
    fn default() -> Self {
        WildCardContainer::List(Vec::new())
    }
}

impl<T: fmt::Display> fmt::Display for WildCardContainer<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WildCardContainer::Wildcard => write!(f, "*"),
            WildCardContainer::List(data) => {
                write!(f, "[")?;
                for (i, item) in data.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
        }
    }
}

impl WildCardContainer<String> {
    /// Converts from a VM stack item.
    pub fn from_stack_item(item: &StackItem) -> Result<Self, String> {
        match item {
            StackItem::Null => Ok(Self::create_wildcard()),
            StackItem::Array(array) => {
                let mut values = Vec::with_capacity(array.len());
                for element in array.items() {
                    let bytes = element
                        .as_bytes()
                        .map_err(|_| "Expected byte string element".to_string())?;
                    let value = String::from_utf8(bytes)
                        .map_err(|_| "Invalid UTF-8 string in wildcard container".to_string())?;
                    values.push(value);
                }
                Ok(Self::create(values))
            }
            StackItem::Struct(struct_item) => {
                // Treat struct the same as array for compatibility.
                let mut values = Vec::with_capacity(struct_item.len());
                for element in struct_item.items() {
                    let bytes = element
                        .as_bytes()
                        .map_err(|_| "Expected byte string element".to_string())?;
                    let value = String::from_utf8(bytes)
                        .map_err(|_| "Invalid UTF-8 string in wildcard container".to_string())?;
                    values.push(value);
                }
                Ok(Self::create(values))
            }
            _ => Err("Unsupported stack item for wildcard container".to_string()),
        }
    }

    /// Converts the container to a VM stack item.
    pub fn to_stack_item(&self) -> StackItem {
        match self {
            Self::Wildcard => StackItem::null(),
            Self::List(values) => {
                let items: Vec<StackItem> = values
                    .iter()
                    .map(|value| StackItem::from_byte_string(value.as_bytes()))
                    .collect();
                StackItem::from_array(items)
            }
        }
    }
}

fn serialize_wildcard<S>(serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str("*")
}

fn deserialize_wildcard<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s == "*" {
        Ok(())
    } else {
        Err(serde::de::Error::custom("Expected '*' for wildcard"))
    }
}
