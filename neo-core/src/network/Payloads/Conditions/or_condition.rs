use std::io::{Read, Write};
use NeoRust::prelude::{Secp256r1PublicKey, VarSizeTrait};
use serde::Deserialize;
use neo_vm::reference_counter::ReferenceCounter;
use neo_vm::stack_item::StackItem;
use crate::io::binary_reader::BinaryReader;
use crate::io::binary_writer::BinaryWriter;
use crate::io::memory_reader::MemoryReader;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::call_flags::CallFlags;
use crate::network::Payloads::Conditions::{WitnessCondition, WitnessConditionType};

#[derive(Debug)]
pub struct OrCondition {
    /// The expressions of the condition.
    pub expressions: Vec<Box<dyn WitnessCondition>>,
}

impl WitnessCondition for OrCondition {
    fn condition_type(&self) -> WitnessConditionType {
        WitnessConditionType::Or
    }

    fn size(&self) -> usize {
        self.base_size() + self.expressions.var_size()
    }

    fn deserialize_without_type(&mut self, reader: &mut BinaryReader, max_nest_depth: usize) -> std::io::Result<()> {
        if max_nest_depth == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Max nest depth exceeded"));
        }
        self.expressions = Self::deserialize_conditions(reader, max_nest_depth - 1)?;
        if self.expressions.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Empty expressions"));
        }
        Ok(())
    }

    fn match_condition(&self, engine: &ApplicationEngine) -> bool {
        self.expressions.iter().any(|p| p.match_condition(engine))
    }

    fn serialize_without_type(&self, writer: &mut BinaryWriter) -> std::io::Result<()> {
        writer.write_var_bytes(&self.expressions)?;
        Ok(())
    }

    fn parse_json(&mut self, json: &JsonObject, max_nest_depth: usize) -> std::io::Result<()> {
        if max_nest_depth == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Max nest depth exceeded"));
        }
        let expressions = json["expressions"].as_array()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid JSON format"))?;
        if expressions.len() > Self::MAX_SUBITEMS {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Too many subitems"));
        }
        self.expressions = expressions.iter()
            .map(|p| Self::from_json(p.as_object().unwrap(), max_nest_depth - 1))
            .collect::<std::io::Result<Vec<_>>>()?;
        if self.expressions.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Empty expressions"));
        }
        Ok(())
    }

    fn to_json(&self) -> JsonValue {
        let mut json = self.base_to_json();
        json["expressions"] = JsonValue::Array(self.expressions.iter().map(|p| p.to_json()).collect());
        json
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> StackItem {
        let mut result = self.base_to_stack_item(reference_counter);
        if let StackItem::Array(array) = &mut result {
            let expressions = Array::new(reference_counter);
            for expr in &self.expressions {
                expressions.add(expr.to_stack_item(reference_counter));
            }
            array.add(StackItem::Array(expressions));
        }
        result
    }
}
