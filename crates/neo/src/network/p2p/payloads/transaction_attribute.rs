// Copyright (C) 2015-2025 The Neo Project.
//
// transaction_attribute.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::{
    conflicts::Conflicts, high_priority_attribute::HighPriorityAttribute,
    not_valid_before::NotValidBefore, notary_assisted::NotaryAssisted,
    oracle_response::OracleResponse, transaction::Transaction,
    transaction_attribute_type::TransactionAttributeType,
};
use crate::neo_io::{MemoryReader, Serializable};
use crate::persistence::DataCache;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

/// Represents an attribute of a transaction.
/// Matches C# TransactionAttribute abstract class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionAttribute {
    /// High priority attribute
    HighPriority(HighPriorityAttribute),
    /// Oracle response
    OracleResponse(OracleResponse),
    /// Not valid before attribute
    NotValidBefore(NotValidBefore),
    /// Conflicts attribute
    Conflicts(Conflicts),
    /// Notary assisted attribute
    NotaryAssisted(NotaryAssisted),
}

impl TransactionAttribute {
    /// Gets the type of the attribute.
    /// Matches C# Type property.
    pub fn get_type(&self) -> TransactionAttributeType {
        match self {
            Self::HighPriority(_) => TransactionAttributeType::HighPriority,
            Self::OracleResponse(_) => TransactionAttributeType::OracleResponse,
            Self::NotValidBefore(_) => TransactionAttributeType::NotValidBefore,
            Self::Conflicts(_) => TransactionAttributeType::Conflicts,
            Self::NotaryAssisted(_) => TransactionAttributeType::NotaryAssisted,
        }
    }

    /// Alias for get_type() to match C# naming.
    pub fn attribute_type(&self) -> TransactionAttributeType {
        self.get_type()
    }

    /// Indicates whether multiple instances of this attribute are allowed.
    /// Matches C# AllowMultiple property.
    pub fn allow_multiple(&self) -> bool {
        match self {
            Self::Conflicts(_) => true,
            _ => false,
        }
    }

    /// Verify the attribute.
    /// Matches C# Verify method.
    pub fn verify(&self, snapshot: &DataCache, tx: &Transaction) -> bool {
        // TODO: Implement verification for each attribute type
        true
    }

    /// Calculate the network fee for this attribute.
    /// Matches C# CalculateNetworkFee method.
    pub fn calculate_network_fee(&self, snapshot: &DataCache, tx: &Transaction) -> i64 {
        // TODO: Get attribute fee from Policy contract
        // NativeContract.Policy.GetAttributeFeeV1(snapshot, (byte)Type)
        0
    }

    /// Converts the attribute to a JSON object.
    /// Matches C# ToJson method.
    pub fn to_json(&self) -> serde_json::Value {
        let mut json = serde_json::json!({
            "type": self.get_type().to_string(),
        });

        // TODO: Add attribute-specific fields
        match self {
            Self::OracleResponse(attr) => {
                if let Some(obj) = json.as_object_mut() {
                    obj.insert("id".to_string(), serde_json::json!(attr.id));
                    obj.insert("code".to_string(), serde_json::json!(attr.code));
                    obj.insert(
                        "result".to_string(),
                        serde_json::json!(hex::encode(&attr.result)),
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

impl Serializable for TransactionAttribute {
    fn size(&self) -> usize {
        1 + match self {
            Self::HighPriority(attr) => attr.size(),
            Self::OracleResponse(attr) => attr.size(),
            Self::NotValidBefore(attr) => attr.size(),
            Self::Conflicts(attr) => attr.size(),
            Self::NotaryAssisted(attr) => attr.size(),
        }
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(&[self.get_type() as u8])?;
        match self {
            Self::HighPriority(attr) => attr.serialize_without_type(writer),
            Self::OracleResponse(attr) => attr.serialize_without_type(writer),
            Self::NotValidBefore(attr) => attr.serialize_without_type(writer),
            Self::Conflicts(attr) => attr.serialize_without_type(writer),
            Self::NotaryAssisted(attr) => attr.serialize_without_type(writer),
        }
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        Self::deserialize_from(reader)
    }
}

impl TransactionAttribute {
    /// Deserializes a TransactionAttribute from a reader.
    /// Matches C# DeserializeFrom static method.
    pub fn deserialize_from(reader: &mut MemoryReader) -> Result<Self, String> {
        let type_byte = reader.read_u8().map_err(|e| e.to_string())?;
        let attr_type = TransactionAttributeType::from_byte(type_byte)
            .ok_or_else(|| format!("Invalid attribute type: {}", type_byte))?;

        match attr_type {
            TransactionAttributeType::HighPriority => Ok(Self::HighPriority(
                HighPriorityAttribute::deserialize(reader)?,
            )),
            TransactionAttributeType::OracleResponse => {
                Ok(Self::OracleResponse(OracleResponse::deserialize(reader)?))
            }
            TransactionAttributeType::NotValidBefore => {
                Ok(Self::NotValidBefore(NotValidBefore::deserialize(reader)?))
            }
            TransactionAttributeType::Conflicts => {
                Ok(Self::Conflicts(Conflicts::deserialize(reader)?))
            }
            TransactionAttributeType::NotaryAssisted => {
                Ok(Self::NotaryAssisted(NotaryAssisted::deserialize(reader)?))
            }
        }
    }
}
