use neo_base::encoding::{write_varint, NeoEncode, NeoWrite};

use super::super::WitnessCondition;

impl NeoEncode for WitnessCondition {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u8(self.kind() as u8);
        match self {
            WitnessCondition::Boolean { expression } => writer.write_u8(*expression as u8),
            WitnessCondition::Not { expression } => expression.neo_encode(writer),
            WitnessCondition::And { expressions } | WitnessCondition::Or { expressions } => {
                write_varint(writer, expressions.len() as u64);
                for expr in expressions {
                    expr.neo_encode(writer);
                }
            }
            WitnessCondition::ScriptHash { hash } | WitnessCondition::CalledByContract { hash } => {
                hash.neo_encode(writer)
            }
            WitnessCondition::Group { group } | WitnessCondition::CalledByGroup { group } => {
                group.neo_encode(writer)
            }
            WitnessCondition::CalledByEntry => {}
        }
    }
}
