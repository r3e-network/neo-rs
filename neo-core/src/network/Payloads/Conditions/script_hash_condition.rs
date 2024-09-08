use std::io::{Read, Write};
use NeoRust::prelude::Secp256r1PublicKey;
use serde::Deserialize;
use neo_vm::reference_counter::ReferenceCounter;
use neo_vm::stack_item::StackItem;
use crate::io::binary_writer::BinaryWriter;
use crate::io::iserializable::ISerializable;
use crate::io::memory_reader::MemoryReader;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::call_flags::CallFlags;
use crate::network::Payloads::Conditions::{WitnessCondition, WitnessConditionType};
use crate::uint160::UInt160;

#[derive(Debug)]
pub struct ScriptHashCondition {
    /// The script hash to be checked.
    pub hash: UInt160,
}

impl WitnessCondition for ScriptHashCondition {
    fn size(&self) -> usize {
        self.base_size() + UInt160::LEN
    }

    fn condition_type(&self) -> WitnessConditionType {
        WitnessConditionType::ScriptHash
    }

    fn deserialize_without_type(&mut self, reader: &mut MemoryReader, _max_nest_depth: i32) -> std::io::Result<()> {
        self.hash = UInt160::deserialize(reader)?;
        Ok(())
    }

    fn match_condition(&self, engine: &ApplicationEngine) -> bool {
        engine.current_script_hash() == self.hash
    }

    fn serialize_without_type(&self, writer: &mut BinaryWriter) -> std::io::Result<()> {
        self.hash.serialize(writer)
    }

    fn parse_json(&mut self, json: &JObject, _max_nest_depth: i32) -> Result<(), Error> {
        self.hash = UInt160::from_str(json["hash"].as_str().ok_or_else(|| Error::new(ErrorKind::InvalidData, "Missing 'hash' field"))?)
            .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
        Ok(())
    }

    fn to_json(&self) -> JObject {
        let mut json = self.base_to_json();
        json.insert("hash".to_string(), self.hash.to_string().into());
        json
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> StackItem {
        let mut result = self.base_to_stack_item(reference_counter);
        if let StackItem::Array(array) = &mut result {
            array.add(StackItem::ByteString(self.hash.to_vec()));
        }
        result
    }
}
