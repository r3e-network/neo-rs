use std::io::{Error, ErrorKind, Read, Write};
use NeoRust::prelude::ECPoint;
use serde::Deserialize;
use neo_json::jtoken::JToken;
use neo_vm::reference_counter::ReferenceCounter;
use neo_vm::stack_item::StackItem;
use crate::io::binary_writer::BinaryWriter;
use crate::io::memory_reader::MemoryReader;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::call_flags::CallFlags;
use crate::network::payloads::conditions::{WitnessCondition, WitnessConditionType};

#[derive(Debug)]
pub struct NotCondition {
    /// The expression of the condition to be reversed.
    pub expression: Box<dyn WitnessCondition>,
}

impl WitnessCondition for NotCondition {
    fn condition_type(&self) -> WitnessConditionType {
        WitnessConditionType::Not
    }

    fn size(&self) -> usize {
        self.expression.size()
    }

    fn deserialize_without_type(&mut self, reader: &mut MemoryReader, max_nest_depth: i32) -> Result<(), Error> {
        if max_nest_depth <= 0 {
            return Err(Error::new(ErrorKind::InvalidData, "Max nest depth exceeded"));
        }
        self.expression = WitnessCondition::deserialize_from(reader, max_nest_depth - 1)?;
        Ok(())
    }

    fn match_condition(&self, engine: &ApplicationEngine) -> bool {
        !self.expression.match_condition(engine)
    }

    fn serialize_without_type(&self, writer: &mut BinaryWriter) {
        self.expression.serialize(writer);
    }

    fn parse_json(&mut self, json: &JToken, max_nest_depth: i32) -> Result<(), Error> {
        if max_nest_depth <= 0 {
            return Err(Error::new(ErrorKind::InvalidData, "Max nest depth exceeded"));
        }
        self.expression = WitnessCondition::from_json(json.get("expression").unwrap().as_object().unwrap(), max_nest_depth - 1)?;
        Ok(())
    }

    fn to_json(&self) -> JToken {
        JToken::new_object()
            .insert("type".to_string(), self.condition_type().to_string().into())
            .unwrap()
        .insert("expression".to_string(), self.expression.to_json())
            .unwrap()
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> StackItem {
        let mut result = StackItem::new_array();
        result.push(StackItem::new_integer(self.condition_type() as i32));
        result.push(self.expression.to_stack_item(reference_counter));
        result
    }
}
