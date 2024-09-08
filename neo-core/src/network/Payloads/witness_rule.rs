use neo_io::{ISerializable, MemoryReader, BinaryWriter};
use neo_json::{JObject, JValue};
use neo_network_p2p_payloads_conditions::WitnessCondition;
use neo_vm::Types::StackItem;
use neo_vm::ReferenceCounter;
use std::convert::TryFrom;
use std::io;
use crate::io::binary_writer::BinaryWriter;
use crate::io::iserializable::ISerializable;
use crate::io::memory_reader::MemoryReader;
use crate::network::Payloads::Conditions::WitnessCondition;
use super::WitnessRuleAction;

/// The rule used to describe the scope of the witness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WitnessRule {
    /// Indicates the action to be taken if the current context meets with the rule.
    pub action: WitnessRuleAction,

    /// The condition of the rule.
    pub condition: dyn WitnessCondition,
}

impl ISerializable for WitnessRule {
    fn size(&self) -> usize {
        std::mem::size_of::<WitnessRuleAction>() + self.condition.size()
    }

    fn deserialize(&mut self, reader: &mut MemoryReader) -> io::Result<()> {
        self.action = WitnessRuleAction::try_from(reader.read_u8()?)?;
        if self.action != WitnessRuleAction::Allow && self.action != WitnessRuleAction::Deny {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid WitnessRuleAction"));
        }
        self.condition = WitnessCondition::deserialize_from(reader, WitnessCondition::MAX_NESTING_DEPTH)?;
        Ok(())
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> io::Result<()> {
        writer.write_u8(self.action as u8)?;
        self.condition.serialize(writer)
    }
}

impl WitnessRule {
    /// Converts the `WitnessRule` from a JSON object.
    pub fn from_json(json: &JObject) -> io::Result<Self> {
        let action = WitnessRuleAction::try_from(json["action"].as_str().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid action"))?)?;

        if action != WitnessRuleAction::Allow && action != WitnessRuleAction::Deny {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid WitnessRuleAction"));
        }

        Ok(Self {
            action,
            condition: WitnessCondition::from_json(json["condition"].as_object().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid condition"))?, WitnessCondition::MAX_NESTING_DEPTH)?,
        })
    }

    /// Converts the rule to a JSON object.
    pub fn to_json(&self) -> JObject {
        JObject::new()
            .set("action", JValue::from(self.action as u8))
            .set("condition", self.condition.to_json())
    }

    pub fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> StackItem {
        StackItem::Array(vec![
            StackItem::Integer((self.action as u8).into()),
            self.condition.to_stack_item(reference_counter),
        ])
    }
}

// Note: The `FromStackItem` trait implementation is omitted as it's not supported according to the C# code.
