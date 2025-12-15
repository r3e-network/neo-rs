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
    oracle_response::OracleResponse, oracle_response_code::OracleResponseCode,
    transaction::Transaction, TransactionAttributeType,
};
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::PolicyContract;
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};

/// Represents an attribute of a transaction.
/// Matches C# TransactionAttribute abstract class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionAttribute {
    /// High priority attribute
    HighPriority,
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
    /// Convenience constructor for the high priority attribute.
    pub fn high_priority() -> Self {
        Self::HighPriority
    }

    /// Convenience constructor for an oracle response with default success code and empty result.
    pub fn oracle_response(id: u64) -> Self {
        Self::OracleResponse(OracleResponse::new(
            id,
            OracleResponseCode::Success,
            Vec::new(),
        ))
    }

    /// Convenience constructor for a "not valid before" attribute.
    pub fn not_valid_before(height: u32) -> Self {
        Self::NotValidBefore(NotValidBefore::new(height))
    }

    /// Gets the type of the attribute.
    /// Matches C# Type property.
    pub fn get_type(&self) -> TransactionAttributeType {
        match self {
            Self::HighPriority => TransactionAttributeType::HighPriority,
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
        matches!(self, Self::Conflicts(_))
    }

    /// Verify the attribute.
    /// Matches C# Verify method.
    pub fn verify(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        tx: &Transaction,
    ) -> bool {
        match self {
            Self::HighPriority => HighPriorityAttribute.verify(settings, snapshot, tx),
            Self::OracleResponse(attr) => attr.verify(settings, snapshot, tx),
            Self::NotValidBefore(attr) => attr.verify(settings, snapshot, tx),
            Self::Conflicts(attr) => attr.verify(settings, snapshot, tx),
            Self::NotaryAssisted(attr) => attr.verify(settings, snapshot, tx),
        }
    }

    /// Calculate the network fee for this attribute.
    /// Matches C# CalculateNetworkFee method.
    pub fn calculate_network_fee(&self, snapshot: &DataCache, tx: &Transaction) -> i64 {
        let policy = PolicyContract::new();
        let base_fee = policy
            .get_attribute_fee_snapshot(snapshot, self.get_type())
            .unwrap_or(PolicyContract::DEFAULT_ATTRIBUTE_FEE as i64);

        match self {
            Self::Conflicts(attr) => attr.calculate_network_fee(base_fee, tx),
            Self::NotaryAssisted(attr) => attr.calculate_network_fee(base_fee, tx),
            _ => base_fee,
        }
    }

    /// Converts the attribute to a JSON object.
    /// Matches C# ToJson method.
    pub fn to_json(&self) -> serde_json::Value {
        let mut json = serde_json::json!({
            "type": self.get_type().to_string(),
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

impl Serializable for TransactionAttribute {
    fn size(&self) -> usize {
        1 + match self {
            Self::HighPriority => 0,
            Self::OracleResponse(attr) => attr.size(),
            Self::NotValidBefore(attr) => attr.size(),
            Self::Conflicts(attr) => attr.size(),
            Self::NotaryAssisted(attr) => attr.size(),
        }
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u8(self.get_type() as u8)?;
        match self {
            Self::HighPriority => Ok(()),
            Self::OracleResponse(attr) => attr.serialize_without_type(writer),
            Self::NotValidBefore(attr) => attr.serialize_without_type(writer),
            Self::Conflicts(attr) => attr.serialize_without_type(writer),
            Self::NotaryAssisted(attr) => attr.serialize_without_type(writer),
        }
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        Self::deserialize_from(reader)
    }
}

impl TransactionAttribute {
    /// Deserializes a TransactionAttribute from a reader.
    /// Matches C# DeserializeFrom static method.
    pub fn deserialize_from(reader: &mut MemoryReader) -> IoResult<Self> {
        let type_byte = reader.read_u8()?;
        let attr_type = TransactionAttributeType::from_byte(type_byte).ok_or_else(|| {
            IoError::invalid_data(format!("Invalid attribute type: {}", type_byte))
        })?;

        match attr_type {
            TransactionAttributeType::HighPriority => Ok(Self::HighPriority),
            TransactionAttributeType::OracleResponse => Ok(Self::OracleResponse(
                <OracleResponse as Serializable>::deserialize(reader)?,
            )),
            TransactionAttributeType::NotValidBefore => Ok(Self::NotValidBefore(
                <NotValidBefore as Serializable>::deserialize(reader)?,
            )),
            TransactionAttributeType::Conflicts => Ok(Self::Conflicts(
                <Conflicts as Serializable>::deserialize(reader)?,
            )),
            TransactionAttributeType::NotaryAssisted => Ok(Self::NotaryAssisted(
                <NotaryAssisted as Serializable>::deserialize(reader)?,
            )),
        }
    }
}
