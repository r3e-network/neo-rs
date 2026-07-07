//! Transaction attribute wire serialization and type-byte dispatch.

use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};

use super::{Conflicts, NotValidBefore, NotaryAssisted, OracleResponse, TransactionAttribute};
use crate::TransactionAttributeType;

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
