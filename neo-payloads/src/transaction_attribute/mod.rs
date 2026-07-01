//! # neo-payloads::transaction_attribute
//!
//! Transaction attribute records and validation helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-payloads`. This protocol crate owns payload
//! records and validation helpers and must not perform IO, storage commits, or
//! service orchestration.
//!
//! ## Contents
//!
//! - `conflicts`: transaction conflict attribute records.
//! - `high_priority_attribute`: high-priority transaction attribute records.
//! - `not_valid_before`: NotValidBefore transaction attribute records.
//! - `notary_assisted`: NotaryAssisted transaction attribute records.
//! - `oracle_response`: OracleResponse transaction attribute records.
//! - `tests`: Module-local tests and regression coverage.

/// Conflicting transaction reference attribute.
pub mod conflicts;
/// High-priority transaction marker attribute.
pub mod high_priority_attribute;
/// Height gate for transaction validity.
pub mod not_valid_before;
/// Notary-assisted transaction attribute.
pub mod notary_assisted;
/// Oracle response transaction attribute.
pub mod oracle_response;

use self::{
    conflicts::Conflicts, not_valid_before::NotValidBefore, notary_assisted::NotaryAssisted,
    oracle_response::OracleResponse,
};
use crate::{OracleResponseCode, TransactionAttributeType, transaction::Transaction};
use base64::{Engine as _, engine::general_purpose};
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_storage::{DataCache, StorageKey};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use serde::{Deserialize, Serialize};

const POLICY_CONTRACT_ID: i32 = -7;
const POLICY_PREFIX_ATTRIBUTE_FEE: u8 = 20;
const DEFAULT_ATTRIBUTE_FEE: i64 = 0;

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

    // verify: Matches C# Verify method. Handled by attribute-type dispatch.

    /// Calculate the network fee for this attribute.
    /// Matches C# CalculateNetworkFee method.
    pub fn calculate_network_fee(&self, snapshot: &DataCache, tx: &Transaction) -> i64 {
        let base = policy_attribute_fee(snapshot, self.type_id());
        match self {
            Self::Conflicts(_) => tx.signers().len() as i64 * base,
            Self::NotaryAssisted(attr) => (i64::from(attr.nkeys) + 1) * base,
            _ => base,
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

fn policy_attribute_fee(snapshot: &DataCache, attribute_type: TransactionAttributeType) -> i64 {
    let key = StorageKey::new(
        POLICY_CONTRACT_ID,
        vec![POLICY_PREFIX_ATTRIBUTE_FEE, attribute_type.to_byte()],
    );
    snapshot
        .get(&key)
        .and_then(|item| BigInt::from_signed_bytes_le(&item.value_bytes()).to_i64())
        .unwrap_or(DEFAULT_ATTRIBUTE_FEE)
}

#[cfg(test)]
#[path = "../tests/transaction_attribute/transaction_attribute.rs"]
mod tests;
