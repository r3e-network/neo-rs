use std::io::{Error, ErrorKind, Read, Write};
use neo_json::jtoken::JToken;
use neo_vm::References;
use neo_vm::stack_item::StackItem;
use crate::io::binary_writer::BinaryWriter;
use crate::io::serializable_trait::SerializableTrait;
use crate::io::memory_reader::MemoryReader;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::network::payloads::conditions::{WitnessCondition, WitnessConditionType};
use neo_type::H160;

#[derive(Debug)]
pub struct ScriptHashCondition {
    /// The script hash to be checked.
    pub hash: H160,
}

impl WitnessCondition for ScriptHashCondition {
    fn condition_type(&self) -> WitnessConditionType {
        WitnessConditionType::ScriptHash
    }

    fn size(&self) -> usize {
        self.base_size() + H160::LEN
    }

    fn deserialize_without_type(&mut self, reader: &mut MemoryReader, _max_nest_depth: i32) -> std::io::Result<()> {
        self.hash = H160::deserialize(reader)?;
        Ok(())
    }

    fn match_condition(&self, engine: &ApplicationEngine) -> bool {
        engine.current_script_hash() == self.hash
    }

    fn serialize_without_type(&self, writer: &mut BinaryWriter) -> std::io::Result<()> {
        self.hash.serialize(writer)
    }

    fn parse_json(&mut self, json: &JToken, _max_nest_depth: i32) -> Result<(), Error> {
        self.hash = H160::from_str(json["hash"].as_str().ok_or_else(|| Error::new(ErrorKind::InvalidData, "Missing 'hash' field"))?)
            .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
        Ok(())
    }

    fn to_json(&self) -> serde_json::Value {
        let mut json = self.base_to_json();
        json.insert("hash".to_string(), self.hash.to_string().into());
        json
    }

    fn to_stack_item(&self, reference_counter: &mut References) -> StackItem {
        let mut result = self.base_to_stack_item(reference_counter);
        if let StackItem::Array(array) = &mut result {
            array.add(StackItem::ByteString(self.hash.to_vec()));
        }
        result
    }
}
