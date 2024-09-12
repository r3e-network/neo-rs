use core::str::FromStr;
use std::io::Write;
use neo_json::jtoken::JToken;
use neo_vm::reference_counter::ReferenceCounter;
use neo_vm::stack_item::StackItem;
use crate::io::memory_reader::MemoryReader;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::network::payloads::conditions::{WitnessCondition, WitnessConditionType};
use crate::uint160::UInt160;

#[derive(Debug)]
pub struct CalledByContractCondition {
    /// The script hash to be checked.
    pub hash: UInt160,
}

impl WitnessCondition for CalledByContractCondition {
    fn condition_type(&self) -> WitnessConditionType {
        WitnessConditionType::CalledByContract
    }

    fn size(&self) -> usize {
        self.base_size() + UInt160::LEN
    }

    fn deserialize_without_type(&mut self, reader: &mut MemoryReader, max_nest_depth: usize) {
        self.hash = UInt160::read_from(reader);
    }

    fn match_condition(&self, engine: &ApplicationEngine) -> bool {
        engine.calling_script_hash().unwrap() == self.hash
    }

    fn serialize_without_type<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(self.hash.as_bytes())
    }

    fn parse_json(&mut self, json: &JToken, max_nest_depth: usize) {
        self.hash = UInt160::from_str(json["hash"].as_str().unwrap()).unwrap();
    }

    fn to_json(&self) -> JToken {
        let mut json = self.base_to_json();
        json.insert("hash".to_string(), self.hash.to_string().into());
        json
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> StackItem {
        let mut result = self.base_to_stack_item(reference_counter);
        if let StackItem::Array(array) = &mut result {
            array.push(StackItem::ByteString(self.hash.to_vec()));
        }
        result
    }
}
