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
    TransactionAttributeType, conflicts::Conflicts, high_priority_attribute::HighPriorityAttribute,
    not_valid_before::NotValidBefore, notary_assisted::NotaryAssisted,
    oracle_response::OracleResponse, oracle_response_code::OracleResponseCode,
    transaction::Transaction,
};
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::PolicyContract;
use base64::{Engine as _, engine::general_purpose};
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

macro_rules! impl_transaction_attribute_wire {
    (
        unit { $unit_variant:ident => $unit_type:ident; }
        typed { $($variant:ident($payload:ty) => $attr_type:ident;)+ }
    ) => {
        impl TransactionAttribute {
            /// Gets the type of the attribute.
            /// Matches C# Type property.
            pub fn type_id(&self) -> TransactionAttributeType {
                match self {
                    Self::$unit_variant => TransactionAttributeType::$unit_type,
                    $(
                        Self::$variant(_) => TransactionAttributeType::$attr_type,
                    )+
                }
            }

            fn body_size(&self) -> usize {
                match self {
                    Self::$unit_variant => 0,
                    $(
                        Self::$variant(attr) => attr.size(),
                    )+
                }
            }

            fn serialize_body(&self, writer: &mut BinaryWriter) -> IoResult<()> {
                match self {
                    Self::$unit_variant => Ok(()),
                    $(
                        Self::$variant(attr) => attr.serialize_without_type(writer),
                    )+
                }
            }

            /// Deserializes a TransactionAttribute from a reader.
            /// Matches C# DeserializeFrom static method.
            pub fn deserialize_from(reader: &mut MemoryReader) -> IoResult<Self> {
                let type_byte = reader.read_u8()?;
                let attr_type = TransactionAttributeType::from_byte(type_byte).ok_or_else(|| {
                    IoError::invalid_data(format!("Invalid attribute type: {type_byte}"))
                })?;

                match attr_type {
                    TransactionAttributeType::$unit_type => Ok(Self::$unit_variant),
                    $(
                        TransactionAttributeType::$attr_type => {
                            Ok(Self::$variant(<$payload as Serializable>::deserialize(reader)?))
                        }
                    )+
                }
            }
        }

        impl Serializable for TransactionAttribute {
            fn size(&self) -> usize {
                1 + self.body_size()
            }

            fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
                writer.write_u8(self.type_id().to_byte())?;
                self.serialize_body(writer)
            }

            fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
                Self::deserialize_from(reader)
            }
        }
    };
}

impl_transaction_attribute_wire! {
    unit { HighPriority => HighPriority; }
    typed {
        OracleResponse(OracleResponse) => OracleResponse;
        NotValidBefore(NotValidBefore) => NotValidBefore;
        Conflicts(Conflicts) => Conflicts;
        NotaryAssisted(NotaryAssisted) => NotaryAssisted;
    }
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

    /// Alias for type_id() to match C# naming.
    pub fn attribute_type(&self) -> TransactionAttributeType {
        self.type_id()
    }

    /// Indicates whether multiple instances of this attribute are allowed.
    /// Matches C# AllowMultiple property.
    pub fn allow_multiple(&self) -> bool {
        self.type_id().allows_multiple()
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
            .get_attribute_fee_snapshot(snapshot, self.type_id())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UInt256;

    fn sample_attributes() -> Vec<TransactionAttribute> {
        vec![
            TransactionAttribute::HighPriority,
            TransactionAttribute::OracleResponse(OracleResponse::new(
                7,
                OracleResponseCode::Success,
                vec![1, 2, 3],
            )),
            TransactionAttribute::NotValidBefore(NotValidBefore::new(42)),
            TransactionAttribute::Conflicts(Conflicts::new(UInt256::from([0xAA; 32]))),
            TransactionAttribute::NotaryAssisted(NotaryAssisted::new(2)),
        ]
    }

    #[test]
    fn wire_mapping_preserves_attribute_type_bytes() {
        for attribute in sample_attributes() {
            let expected_type = attribute.type_id();
            let mut writer = BinaryWriter::new();

            Serializable::serialize(&attribute, &mut writer).unwrap();
            let bytes = writer.into_bytes();

            assert_eq!(bytes[0], expected_type.to_byte(), "{attribute:?}");

            let mut reader = MemoryReader::new(&bytes);
            let decoded = TransactionAttribute::deserialize_from(&mut reader).unwrap();

            assert_eq!(decoded.type_id(), expected_type);
            assert_eq!(reader.remaining(), 0);
        }
    }

    #[test]
    fn multiplicity_matches_attribute_type_table() {
        for attribute in sample_attributes() {
            assert_eq!(
                attribute.allow_multiple(),
                attribute.type_id().allows_multiple(),
                "{attribute:?}"
            );
        }

        assert!(TransactionAttribute::Conflicts(Conflicts::new(UInt256::zero())).allow_multiple());
        assert!(!TransactionAttribute::NotaryAssisted(NotaryAssisted::new(1)).allow_multiple());
    }
}
