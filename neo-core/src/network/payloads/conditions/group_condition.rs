use std::io::{Read, Write};
use NeoRust::prelude::{NeoSerializable};
use serde::Deserialize;
use crate::cryptography::ECPoint;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::call_flags::CallFlags;
use crate::network::payloads::conditions::{WitnessCondition, WitnessConditionType};

#[derive(Debug)]
pub struct GroupCondition {
    /// The group to be checked.
    pub group: ECPoint,
}

impl WitnessCondition for GroupCondition {
    fn condition_type(&self) -> WitnessConditionType {
        WitnessConditionType::Group
    }

    fn size(&self) -> usize {
        self.base_size() + self.group.size()
    }

    fn deserialize_without_type(&mut self, reader: &mut dyn Read, max_nest_depth: i32) -> std::io::Result<()> {
        self.group = ECPoint::deserialize(reader)?;
        Ok(())
    }

    fn match_condition(&self, engine: &mut ApplicationEngine) -> bool {
        engine.validate_call_flags(CallFlags::READ_STATES);
        if let Some(contract) = NativeContract::contract_management().get_contract(engine.snapshot_cache(), engine.current_script_hash()) {
            contract.manifest.groups.iter().any(|p| p.pub_key == self.group)
        } else {
            false
        }
    }

    fn serialize_without_type(&self, writer: &mut dyn Write) -> std::io::Result<()> {
        self.group.serialize(writer)
    }

    fn parse_json(&mut self, json: &JsonValue, max_nest_depth: i32) -> Result<(), json::Error> {
        self.group = ECPoint::from_str(json["group"].as_str().ok_or(json::Error::WrongType)?)
            .map_err(|_| json::Error::WrongType)?;
        Ok(())
    }

    fn to_json(&self) -> JsonValue {
        let mut json = json::object! {
            "type": self.condition_type().to_string(),
            "group": self.group.to_string(),
        };
        json
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> StackItem {
        let mut result = Array::new(reference_counter);
        result.push(StackItem::Integer(self.condition_type() as i32));
        result.push(StackItem::ByteString(self.group.to_array()));
        StackItem::Array(result)
    }
}

impl GroupCondition {
    pub fn new(group: ECPoint) -> Self {
        Self { group }
    }
}
