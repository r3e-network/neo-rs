use crate::interop::{Hash160, PublicKey};

// TransactionSigner represents the signer of a NEO transaction. It's similar to
// the Signer class in the Neo .NET framework.
pub struct TransactionSigner {
    // Account represents the account (160 bit BE value in a 20 byte slice) of
    // the given signer.
    pub account: Hash160,
    // Scopes represents a set of witness flags for the given signer.
    pub scopes: SignerScope,
    // Contracts represents the set of contract hashes (160 bit BE value in a 20
    // byte slice) allowed to be called by the signer. It is only non-empty if
    // CustomContracts scope flag is set.
    pub allowed_contracts: Vec<Hash160>,
    // AllowedGroups represents the set of contract groups (ecdsa public key
    // bytes in a 33 byte slice) allowed to be called by the signer. It is only
    // non-empty if CustomGroups scope flag is set.
    pub allowed_groups: Vec<PublicKey>,
    // Rules represents a rule-based witness scope of the given signer. It is
    // only non-empty if Rules scope flag is set.
    pub rules: Vec<WitnessRule>,
}

// SignerScope represents a signer's witness scope.
#[derive(Clone, Copy)]
pub enum SignerScope {
    // None specifies that no contract was witnessed. Only signs the transaction
    // and pays GAS fee if a sender.
    None = 0,
    // CalledByEntry means that the witness is valid only when the witness
    // checking contract is called from the entry script.
    CalledByEntry = 0x01,
    // CustomContracts define custom hash for contract-specific witness.
    CustomContracts = 0x10,
    // CustomGroups define custom public key for group members.
    CustomGroups = 0x20,
    // Rules is a set of conditions with boolean operators.
    Rules = 0x40,
    // Global allows this witness in all contexts. This cannot be combined with
    // other flags.
    Global = 0x80,
}

// WitnessRule represents a single rule for Rules witness scope.
pub struct WitnessRule {
    // Action denotes whether the witness condition should be accepted or denied.
    pub action: WitnessAction,
    // Condition holds a set of nested witness rules. Max nested depth is 2.
    pub condition: WitnessCondition,
}

// WitnessAction represents an action to perform in WitnessRule if
// witness condition matches.
#[derive(Clone, Copy)]
pub enum WitnessAction {
    // WitnessDeny rejects current witness if condition is met.
    WitnessDeny = 0,
    // WitnessAllow approves current witness if condition is met.
    WitnessAllow = 1,
}

// WitnessCondition represents a single witness condition for a rule-based
// witness. Its type can always be safely accessed, but trying to access its
// value causes runtime exception for those types that don't have value
// (currently, it's only CalledByEntry witness condition).
pub struct WitnessCondition {
    pub condition_type: WitnessConditionType,
    // Depends on the witness condition Type, its value can be asserted to the
    // certain structure according to the following rule:
    // WitnessBoolean -> bool
    // WitnessNot ->  Vec<WitnessCondition> with one element
    // WitnessAnd -> Vec<WitnessCondition>
    // WitnessOr -> Vec<WitnessCondition>
    // WitnessScriptHash -> Hash160
    // WitnessGroup -> PublicKey
    // WitnessCalledByContract -> Hash160
    // WitnessCalledByGroup -> PublicKey
    // WitnessCalledByEntry -> doesn't have value, thus, an attempt to access the Value leads to runtime exception.
    pub value: Option<WitnessConditionValue>,
}

pub enum WitnessConditionValue {
    Boolean(bool),
    Not(Box<WitnessCondition>),
    And(Vec<WitnessCondition>),
    Or(Vec<WitnessCondition>),
    ScriptHash(Hash160),
    Group(PublicKey),
    CalledByContract(Hash160),
    CalledByGroup(PublicKey),
}

// WitnessConditionType represents the type of rule-based witness condition.
#[derive(Clone, Copy)]
pub enum WitnessConditionType {
    // WitnessBoolean is a generic boolean condition.
    WitnessBoolean = 0x00,
    // WitnessNot reverses another condition.
    WitnessNot = 0x01,
    // WitnessAnd means that all conditions must be met.
    WitnessAnd = 0x02,
    // WitnessOr means that any of conditions must be met.
    WitnessOr = 0x03,
    // WitnessScriptHash matches executing contract's script hash.
    WitnessScriptHash = 0x18,
    // WitnessGroup matches executing contract's group key.
    WitnessGroup = 0x19,
    // WitnessCalledByEntry matches when current script is an entry script or is
    // called by an entry script.
    WitnessCalledByEntry = 0x20,
    // WitnessCalledByContract matches when current script is called by the
    // specified contract.
    WitnessCalledByContract = 0x28,
    // WitnessCalledByGroup matches when current script is called by contract
    // belonging to the specified group.
    WitnessCalledByGroup = 0x29,
}
