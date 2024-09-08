use std::io::{Read, Write};
use NeoRust::prelude::Secp256r1PublicKey;
use serde::{Deserialize, Serialize};
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::call_flags::CallFlags;
use crate::network::Payloads::Conditions::{WitnessCondition, WitnessConditionType};

#[derive(Debug)]
pub struct CalledByGroupCondition {
    /// The group to be checked.
    pub group: Secp256r1PublicKey,
}

impl WitnessCondition for CalledByGroupCondition {
    fn size(&self) -> usize {
        self.base_size() + self.group.size()
    }

    fn condition_type(&self) -> WitnessConditionType {
        WitnessConditionType::CalledByGroup
    }

    fn deserialize_without_type(&mut self, reader: &mut dyn Read, max_nest_depth: usize) -> std::io::Result<()> {
        self.group = Secp256r1PublicKey::deserialize(reader)?;
        Ok(())
    }

    fn match_condition(&self, engine: &mut ApplicationEngine) -> bool {
        engine.validate_call_flags(CallFlags::READ_STATES);
        let contract = NativeContract::contract_management().get_contract(engine.snapshot_cache(), engine.calling_script_hash());
        contract.map_or(false, |c| c.manifest.groups.iter().any(|p| p.pub_key == self.group))
    }

    fn serialize_without_type(&self, writer: &mut dyn Write) -> std::io::Result<()> {
        self.group.serialize(writer)
    }

    fn parse_json(&mut self, json: &JsonValue, max_nest_depth: usize) -> Result<(), String> {
        self.group = Secp256r1PublicKey::from_str(json["group"].as_str().ok_or("Missing 'group' field")?)
            .map_err(|e| format!("Invalid Secp256r1PublicKey: {}", e))?;
        Ok(())
    }

    fn to_json(&self) -> JsonValue {
        let mut json = self.base_to_json();
        json["group"] = JsonValue::String(self.group.to_string());
        json
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> StackItem {
        let mut result = self.base_to_stack_item(reference_counter);
        if let StackItem::Array(array) = &mut result {
            array.push(StackItem::ByteString(self.group.to_bytes()));
        }
        result
    }
}

impl CalledByGroupCondition {
    pub fn new(group: Secp256r1PublicKey) -> Self {
        Self { group }
    }
}
