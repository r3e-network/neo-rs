use alloc::rc::Rc;
use core::str::FromStr;
use std::io::{Error, ErrorKind};
use std::fmt;
use neo_json::jtoken::JToken;
use neo_vm::References;
use neo_vm::stack_item::StackItem;
use crate::io::binary_writer::BinaryWriter;
use crate::io::memory_reader::MemoryReader;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::network::payloads::conditions::WitnessConditionType;

pub const MAX_SUBITEMS: usize = 16;
pub const MAX_NESTING_DEPTH: i32 = 2;

pub trait WitnessCondition: fmt::Debug {
    /// The type of the WitnessCondition.
    fn condition_type(&self) -> WitnessConditionType;

    fn size(&self) -> usize {
        std::mem::size_of::<WitnessConditionType>()
    }

    fn deserialize(&mut self, reader: &mut MemoryReader) -> Result<(), Error> {
        let condition_type = WitnessConditionType::from(reader.read_u8()?);
        if condition_type != self.condition_type() {
            return Err(Error::new(ErrorKind::InvalidData, "Invalid condition type"));
        }
        self.deserialize_without_type(reader, MAX_NESTING_DEPTH)
    }

    fn deserialize_conditions(reader: &mut MemoryReader, max_nest_depth: i32) -> Result<Vec<Box<dyn WitnessCondition>>, Error> {
        let count = reader.read_var_int(MAX_SUBITEMS as u64)? as usize;
        let mut conditions = Vec::with_capacity(count);
        for _ in 0..count {
            conditions.push(Self::deserialize_from(reader, max_nest_depth)?);
        }
        Ok(conditions)
    }

    fn deserialize_from(reader: &mut MemoryReader, max_nest_depth: i32) -> Result<Box<dyn WitnessCondition>, Error> {
        let condition_type = WitnessConditionType::from(reader.read_u8()?);
        let mut condition = condition_type.create_instance()?;
        condition.deserialize_without_type(reader, max_nest_depth)?;
        Ok(condition)
    }

    fn deserialize_without_type(&mut self, reader: &mut MemoryReader, max_nest_depth: i32) -> Result<(), Error>;

    /// Checks whether the current context matches the condition.
    fn match_condition(&self, engine: &ApplicationEngine) -> bool;

    fn serialize(&self, writer: &mut BinaryWriter) -> Result<(), Error> {
        writer.write_u8(self.condition_type() as u8)?;
        self.serialize_without_type(writer)
    }

    fn serialize_without_type(&self, writer: &mut BinaryWriter) -> Result<(), Error>;

    fn parse_json(&mut self, json: &JToken, max_nest_depth: i32) -> Result<(), Error>;

    fn from_json(json: &JToken, max_nest_depth: i32) -> Result<Box<dyn WitnessCondition>, Error> {
        let condition_type = WitnessConditionType::from_str(&json["type"].as_string().ok_or_else(|| Error::new(ErrorKind::InvalidData, "Invalid JSON format"))?)?;
        let mut condition = condition_type.create_instance()?;
        condition.parse_json(json, max_nest_depth)?;
        Ok(condition)
    }

    fn to_json(&self) -> JToken {
        let mut json = JToken::new_object();
        json.insert("type", self.condition_type().to_string());
        json
    }

    fn to_stack_item(&self, reference_counter: &mut References) -> StackItem {
        StackItem::Array(vec![Rc::new(StackItem::Integer(self.condition_type()))])
    }
}

impl fmt::Display for dyn WitnessCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
