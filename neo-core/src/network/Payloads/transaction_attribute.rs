use std::io;
use neo_json::jtoken::JToken;
use crate::io::binary_writer::BinaryWriter;
use crate::io::iserializable::ISerializable;
use crate::io::memory_reader::MemoryReader;
use crate::persistence::DataCache;
use super::Transaction;
use super::TransactionAttributeType;

/// Represents an attribute of a transaction.
pub trait TransactionAttribute: ISerializable {
    /// The type of the attribute.
    fn get_type(&self) -> TransactionAttributeType;

    /// Indicates whether multiple instances of this attribute are allowed.
    fn allow_multiple(&self) -> bool;

    fn size(&self) -> usize {
        std::mem::size_of::<TransactionAttributeType>()
    }

    fn deserialize(&mut self, reader: &mut MemoryReader) -> io::Result<()> {
        let attr_type = reader.read_u8()?;
        if attr_type != self.get_type() as u8 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid attribute type"));
        }
        self.deserialize_without_type(reader)
    }

    /// Deserializes an TransactionAttribute object from a MemoryReader.
    fn deserialize_from(reader: &mut MemoryReader) -> io::Result<Box<dyn TransactionAttribute>> {
        let attr_type = TransactionAttributeType::try_from(reader.read_u8()?)?;
        let mut attribute = ReflectionCache::<TransactionAttributeType>::create_instance(attr_type)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid attribute type"))?;
        attribute.deserialize_without_type(reader)?;
        Ok(attribute)
    }

    /// Deserializes the TransactionAttribute object from a MemoryReader.
    fn deserialize_without_type(&mut self, reader: &mut MemoryReader) -> io::Result<()>;

    /// Converts the attribute to a JSON object.
    fn to_json(&self) -> JToken {
        JToken::new_object().insert("type".to_string(), self.get_type() as u8)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> io::Result<()> {
        writer.write_u8(self.get_type() as u8);
        self.serialize_without_type(writer)
    }

    /// Serializes the TransactionAttribute object to a BinaryWriter.
    fn serialize_without_type(&self, writer: &mut BinaryWriter) -> io::Result<()>;

    /// Verifies the attribute with the transaction.
    fn verify(&self, snapshot: &dyn DataCache, tx: &Transaction) -> bool {
        true
    }

    fn calculate_network_fee(&self, snapshot: &dyn DataCache, tx: &Transaction) -> i64 {
        NativeContract::Policy.get_attribute_fee(snapshot, self.get_type() as u8)
    }
}
