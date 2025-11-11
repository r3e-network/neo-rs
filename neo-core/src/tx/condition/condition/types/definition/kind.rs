use alloc::{boxed::Box, vec::Vec};

use neo_base::hash::Hash160;
use neo_crypto::ecc256::PublicKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WitnessCondition {
    Boolean { expression: bool },
    Not { expression: Box<WitnessCondition> },
    And { expressions: Vec<WitnessCondition> },
    Or { expressions: Vec<WitnessCondition> },
    ScriptHash { hash: Hash160 },
    Group { group: PublicKey },
    CalledByEntry,
    CalledByContract { hash: Hash160 },
    CalledByGroup { group: PublicKey },
}
