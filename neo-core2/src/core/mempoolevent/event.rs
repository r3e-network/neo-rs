use serde::{Deserialize, Serialize};
use serde_json;
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

use crate::core::transaction::Transaction;

// Type represents mempool event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Type {
    // TransactionAdded marks transaction addition mempool event.
    #[serde(rename = "added")]
    TransactionAdded = 0x01,
    // TransactionRemoved marks transaction removal mempool event.
    #[serde(rename = "removed")]
    TransactionRemoved = 0x02,
}

// Event represents one of mempool events: transaction was added or removed from the mempool.
#[derive(Debug, Serialize, Deserialize)]
pub struct Event {
    #[serde(rename = "type")]
    pub event_type: Type,
    pub tx: Option<Transaction>,
    pub data: serde_json::Value,
}

// Implementing Display trait for Type
impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::TransactionAdded => write!(f, "added"),
            Type::TransactionRemoved => write!(f, "removed"),
            _ => write!(f, "unknown"),
        }
    }
}

// Custom error type for invalid event type name
#[derive(Debug, Error)]
#[error("invalid event type name")]
pub struct InvalidEventTypeName;

// Implementing FromStr trait for Type
impl FromStr for Type {
    type Err = InvalidEventTypeName;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "added" => Ok(Type::TransactionAdded),
            "removed" => Ok(Type::TransactionRemoved),
            _ => Err(InvalidEventTypeName),
        }
    }
}

// Implementing custom serialization for Type
impl Serialize for Type {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

// Implementing custom deserialization for Type
impl<'de> Deserialize<'de> for Type {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}
