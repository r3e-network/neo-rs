use core::fmt::{Debug};
use neo_json::jtoken::JToken;
use neo_vm::reference_counter::ReferenceCounter;
use neo_vm::stack_item::StackItem;
use crate::io::binary_writer::BinaryWriter;
use crate::io::memory_reader::MemoryReader;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::network::payloads::conditions::{WitnessCondition, WitnessConditionType};

#[derive(Debug)]
pub struct BooleanCondition {
    /// The expression of the BooleanCondition.
    pub expression: bool,
}

impl WitnessCondition for BooleanCondition {
    fn condition_type(&self) -> WitnessConditionType {
        WitnessConditionType::Boolean
    }

    fn size(&self) -> usize {
        <Self as WitnessCondition>::size(self) + std::mem::size_of::<bool>()
    }

    fn deserialize_without_type(&mut self, reader: &mut MemoryReader, _max_nest_depth: i32) {
        self.expression = reader.read_bool().unwrap();
    }

    fn match_condition(&self, _engine: &ApplicationEngine) -> bool {
        self.expression
    }

    fn serialize_without_type(&self, writer: &mut BinaryWriter) {
        writer.write_bool(self.expression);
    }

    fn parse_json(&mut self, json: &JToken, _max_nest_depth: i32) {
        self.expression = json["expression"].as_bool().unwrap();
    }

    fn to_json(&self) -> JToken {
        let mut json = <Self as WitnessCondition>::to_json(self);
        json.insert("expression".to_string(), self.expression.into()).expect("TODO: panic message");
        json
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> StackItem {
        let mut result = <Self as WitnessCondition>::to_stack_item(self, reference_counter);
        if let StackItem::Array(array) = &mut result {
            array.add(StackItem::Boolean(self.expression));
        }
        result
    }
}
