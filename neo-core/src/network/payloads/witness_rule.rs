use std::convert::TryFrom;
use std::io;
use neo_json::jtoken::JToken;
use neo_vm::References;
use neo_vm::stack_item::StackItem;
use crate::io::binary_writer::BinaryWriter;
use crate::io::iserializable::ISerializable;
use crate::io::memory_reader::MemoryReader;
use crate::network::payloads::conditions::{WitnessCondition, MAX_NESTING_DEPTH};
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

    fn serialize(&self, writer: &mut BinaryWriter) {
        writer.write_u8(self.action as u8);
        self.condition.serialize(writer).expect("TODO: panic message");
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, std::io::Error> {
        let action = WitnessRuleAction::try_from(reader.read_u8()?)?;
        if action != WitnessRuleAction::Allow && action != WitnessRuleAction::Deny {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid WitnessRuleAction"));
        }
        let condition = WitnessCondition::deserialize_from(reader, MAX_NESTING_DEPTH)?;
        Ok(Self { action, condition })
    }
}


impl IJsonConvertible for WitnessRule {
     fn from_json(json: &JToken) -> io::Result<Self> {
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
     fn to_json(&self) -> JToken {
        JToken::new_object()
            .insert("action".to_string(), JValue::from(self.action as u8))
            .unwrap()
            .insert("condition".to_string(), self.condition.to_json())

    }
}

impl IInteroperable for WitnessRule {
    type Error;

    fn from_stack_item(stack_item: &Rc<StackItem>) -> Result<Self, Self::Error> {
        let array = stack_item.try_as_array()?;
        let action = WitnessRuleAction::try_from(array[0].try_as_integer()?.as_u64()? as u8)?;
        let condition = WitnessCondition::from_stack_item(&array[1])?;
        Ok(Self { action, condition })  
    }

    fn to_stack_item(&self, reference_counter: Option<&References>) -> Result<Rc<StackItem>, Self::Error> {
        Ok(StackItem::Array(vec![
            StackItem::Integer((self.action as u8).into()),
            self.condition.to_stack_item(reference_counter),
        ]))
    }
}
