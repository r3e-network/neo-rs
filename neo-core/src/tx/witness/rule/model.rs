use crate::tx::condition::{WitnessCondition, WitnessConditionContext};
use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use super::Action;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitnessRule {
    pub action: Action,
    pub condition: WitnessCondition,
}

impl WitnessRule {
    pub fn new(action: Action, condition: WitnessCondition) -> Self {
        Self { action, condition }
    }

    pub fn evaluate(&self, ctx: &WitnessConditionContext<'_>) -> Option<bool> {
        if self.condition.matches(ctx) {
            Some(matches!(self.action, Action::Allow))
        } else {
            None
        }
    }
}

impl NeoEncode for WitnessRule {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u8(self.action as u8);
        self.condition.neo_encode(writer);
    }
}

impl NeoDecode for WitnessRule {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let action_byte = reader.read_u8()?;
        let action = Action::try_from(action_byte)?;
        let condition = WitnessCondition::neo_decode(reader)?;
        Ok(Self { action, condition })
    }
}
