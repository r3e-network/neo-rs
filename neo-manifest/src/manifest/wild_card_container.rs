//! WildCardContainer - matches C# Neo.SmartContract.Manifest.WildCardContainer exactly

use neo_vm::StackItem;
use neo_vm_rs::StackValue;
use serde::{Deserialize, Serialize};
use std::fmt;

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
    fn strings_from_stack_values(items: Vec<StackValue>) -> Result<Vec<String>, String> {
        let mut values = Vec::with_capacity(items.len());
        for element in items {
            let bytes = element
                .to_byte_string_bytes()
                .ok_or_else(|| "Expected byte string element".to_string())?;
            let value = String::from_utf8(bytes)
                .map_err(|_| "Invalid UTF-8 string in wildcard container".to_string())?;
            values.push(value);
        }
        Ok(values)
    }

    /// Converts from a neo-vm-rs stack value.
    pub fn from_stack_value(stack_value: StackValue) -> Result<Self, String> {
        match stack_value {
            StackValue::Null => Ok(Self::create_wildcard()),
            StackValue::Array(items) => Ok(Self::create(Self::strings_from_stack_values(items)?)),
            StackValue::Struct(items) => Ok(Self::create(Self::strings_from_stack_values(items)?)),
            _ => Err("Unsupported stack value for wildcard container".to_string()),
        }
    }

    /// Converts from a VM stack item.
    pub fn from_stack_item(item: &StackItem) -> Result<Self, String> {
        Self::from_stack_value(
            StackValue::try_from(item.clone())
                .map_err(|_| "Unsupported stack item for wildcard container".to_string())?,
        )
    }

    /// Converts the container to a neo-vm-rs stack value.
    pub fn to_stack_value(&self) -> StackValue {
        match self {
            Self::Wildcard => StackValue::Null,
            Self::List(values) => StackValue::Array(
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
mod tests {
    use super::*;
    use neo_vm_rs::StackValue;

    #[test]
    fn string_wildcard_projects_to_neo_vm_rs_null() {
        assert_eq!(
            WildCardContainer::<String>::create_wildcard().to_stack_value(),
            StackValue::Null
        );
    }

    #[test]
    fn string_list_projects_to_neo_vm_rs_byte_string_array() {
        let container = WildCardContainer::create(vec!["transfer".to_string(), "balanceOf".into()]);

        assert_eq!(
            container.to_stack_value(),
            StackValue::Array(vec![
                StackValue::ByteString(b"transfer".to_vec()),
                StackValue::ByteString(b"balanceOf".to_vec()),
            ])
        );
    }

    #[test]
    fn string_stack_item_projection_matches_stack_value_projection() {
        let container = WildCardContainer::create(vec!["deploy".to_string(), "update".into()]);
        let expected = StackItem::try_from(container.to_stack_value()).unwrap();

        assert_eq!(container.to_stack_item(), expected);
    }

    #[test]
    fn string_wildcard_reads_from_neo_vm_rs_null() {
        assert_eq!(
            WildCardContainer::<String>::from_stack_value(StackValue::Null).unwrap(),
            WildCardContainer::Wildcard
        );
    }

    #[test]
    fn string_list_reads_from_neo_vm_rs_array() {
        assert_eq!(
            WildCardContainer::<String>::from_stack_value(StackValue::Array(vec![
                StackValue::ByteString(b"mint".to_vec()),
                StackValue::ByteString(b"burn".to_vec()),
            ]))
            .unwrap(),
            WildCardContainer::create(vec!["mint".to_string(), "burn".into()])
        );
    }

    #[test]
    fn string_list_reads_from_neo_vm_rs_struct_for_compatibility() {
        assert_eq!(
            WildCardContainer::<String>::from_stack_value(StackValue::Struct(vec![
                StackValue::ByteString(b"verify".to_vec()),
                StackValue::ByteString(b"onNEP17Payment".to_vec()),
            ]))
            .unwrap(),
            WildCardContainer::create(vec!["verify".to_string(), "onNEP17Payment".into()])
        );
    }
}
