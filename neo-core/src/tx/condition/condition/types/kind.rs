#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WitnessConditionType {
    Boolean = 0x00,
    Not = 0x01,
    And = 0x02,
    Or = 0x03,
    ScriptHash = 0x18,
    Group = 0x19,
    CalledByEntry = 0x20,
    CalledByContract = 0x28,
    CalledByGroup = 0x29,
}
