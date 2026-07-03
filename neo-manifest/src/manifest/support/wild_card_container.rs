//! WildCardContainer - matches C# Neo.SmartContract.Manifest.WildCardContainer exactly

use neo_error::{CoreError, CoreResult};
use neo_vm::StackItem;
use neo_vm_rs::StackValue;
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::manifest::stack_value_helpers::stack_value_to_utf8_string;

use neo_vm::impl_interoperable_via_stack_value;

/// A list that supports wildcard (matches C# WildcardContainer\<T>)
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
    pub fn from_json(json: &serde_json::Value) -> CoreResult<Self>
    where
        T: for<'de> Deserialize<'de>,
    {
        match json {
            serde_json::Value::String(s) if s == "*" => Ok(WildCardContainer::Wildcard),
            serde_json::Value::Array(_) => {
                let data: Vec<T> = serde_json::from_value(json.clone())
                    .map_err(|e| CoreError::other(e.to_string()))?;
                Ok(WildCardContainer::List(data))
            }
            _ => Err(CoreError::other("Invalid WildCardContainer format")),
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
    fn strings_from_stack_values(items: Vec<StackValue>) -> CoreResult<Vec<String>> {
        let mut values = Vec::with_capacity(items.len());
        for element in items {
            let value = stack_value_to_utf8_string(&element, "Wildcard string element")?;
            values.push(value);
        }
        Ok(values)
    }

    /// Converts from a neo-vm-rs stack value.
    pub fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        match stack_value {
            StackValue::Null => Ok(Self::create_wildcard()),
            StackValue::Array(_, items) => {
                Ok(Self::create(Self::strings_from_stack_values(items)?))
            }
            _ => Err(CoreError::other(
                "Unsupported stack value for wildcard container",
            )),
        }
    }

    /// Converts from a VM stack item.
    pub fn from_stack_item(item: &StackItem) -> CoreResult<Self> {
        Self::from_stack_value(
            StackValue::try_from(item.clone())
                .map_err(|_| CoreError::other("Unsupported stack item for wildcard container"))?,
        )
    }

    /// Converts the container to a neo-vm-rs stack value.
    pub fn to_stack_value(&self) -> StackValue {
        match self {
            Self::Wildcard => StackValue::Null,
            Self::List(values) => StackValue::Array(
                neo_vm_rs::next_stack_item_id(),
                values
                    .iter()
                    .map(|value| StackValue::ByteString(value.as_bytes().to_vec()))
                    .collect(),
            ),
        }
    }

    /// Converts the container to a VM stack item.
    pub fn to_stack_item(&self) -> StackItem {
        StackItem::try_from(self.to_stack_value())
            .expect("wildcard container StackValue projection must be StackItem-compatible")
    }
}

impl_interoperable_via_stack_value!(WildCardContainer<String>);

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

#[cfg(test)]
#[path = "../../tests/manifest/wild_card_container.rs"]
mod tests;
