// Copyright (C) 2015-2025 The Neo Project.
// Licensed under the MIT License. See LICENSE file for details.
use super::{
    Conflicts, NotValidBefore, NotaryAssisted, OracleResponse, OracleResponseCode,
    TransactionAttributeType,
};
use base64::{Engine as _, engine::general_purpose};
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

/// Normative attributes; snapshot verification and fee policy live in the ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionAttribute {
    /// High priority, with no body.
    HighPriority,
    /// An oracle response.
    OracleResponse(OracleResponse),
    /// A first valid height.
    NotValidBefore(NotValidBefore),
    /// A transaction conflict hash.
    Conflicts(Conflicts),
    /// A notary key count.
    NotaryAssisted(NotaryAssisted),
}

macro_rules! impl_transaction_attribute_wire {
    (unit { $unit_variant:ident => $unit_type:ident; }
     typed { $($variant:ident($payload:ty) => $attr_type:ident;)+ }) => {
        impl TransactionAttribute {
            /// Gets the primitive attribute type.
            pub fn type_id(&self) -> TransactionAttributeType {
                match self { Self::$unit_variant => TransactionAttributeType::$unit_type,
                    $(Self::$variant(_) => TransactionAttributeType::$attr_type,)+ }
            }
            fn body_size(&self) -> usize {
                match self { Self::$unit_variant => 0, $(Self::$variant(attr) => attr.size(),)+ }
            }
            fn serialize_body(&self, writer: &mut BinaryWriter) -> IoResult<()> {
                match self { Self::$unit_variant => Ok(()), $(Self::$variant(attr) => attr.serialize_without_type(writer),)+ }
            }
            /// Reads a type byte followed by its body.
            pub fn deserialize_from(reader: &mut MemoryReader) -> IoResult<Self> {
                let type_byte = reader.read_u8()?;
                let attr_type = TransactionAttributeType::from_byte(type_byte)
                    .ok_or_else(|| IoError::invalid_data(format!("Invalid attribute type: {type_byte}")))?;
                match attr_type { TransactionAttributeType::$unit_type => Ok(Self::$unit_variant),
                    $(TransactionAttributeType::$attr_type => Ok(Self::$variant(<$payload as Serializable>::deserialize(reader)?)),)+ }
            }
        }
        impl Serializable for TransactionAttribute {
            fn size(&self) -> usize { 1 + self.body_size() }
            fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
                writer.write_u8(self.type_id().to_byte())?;
                self.serialize_body(writer)
            }
            fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> { Self::deserialize_from(reader) }
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
    /// Creates a high priority attribute.
    pub fn high_priority() -> Self {
        Self::HighPriority
    }
    /// Creates an empty successful oracle response.
    pub fn oracle_response(id: u64) -> Self {
        Self::OracleResponse(OracleResponse::new(
            id,
            OracleResponseCode::Success,
            Vec::new(),
        ))
    }
    /// Creates a first-valid-height attribute.
    pub fn not_valid_before(height: u32) -> Self {
        Self::NotValidBefore(NotValidBefore::new(height))
    }
    /// Alias for the primitive type ID.
    pub fn attribute_type(&self) -> TransactionAttributeType {
        self.type_id()
    }
    /// Whether this type permits multiple instances.
    pub fn allow_multiple(&self) -> bool {
        self.type_id().allows_multiple()
    }
    /// Converts the attribute to a JSON object.
    pub fn to_json(&self) -> serde_json::Value {
        let mut json = serde_json::json!({ "type": self.type_id().to_string() });
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
    use neo_primitives::UInt256;
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
