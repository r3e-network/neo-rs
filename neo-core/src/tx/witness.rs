// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

use neo_base::encoding::bin::*;
use crate::types::Script;


pub const MAX_CONDITION_NESTING: usize = 2;


#[derive(Debug, Clone, Deserialize, Serialize, BinEncode, BinDecode)]
pub struct Witness {
    pub invocation_script: Script,
    pub verification_script: Script,
}

impl Witness {
    pub fn new(invocation: Script, verification: Script) -> Self {
        Self {
            invocation_script: invocation,
            verification_script: verification,
        }
    }
}


#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Witnesses(pub(crate) [Witness; 1]);

impl Witnesses {
    pub fn witness(&self) -> &Witness { &self.0[0] }
}

impl Default for Witnesses {
    fn default() -> Self {
        Self([Witness::new(Script::default(), Script::default())])
    }
}

impl From<Witness> for Witnesses {
    fn from(value: Witness) -> Self { Self([value]) }
}

impl BinEncoder for Witnesses {
    fn encode_bin(&self, w: &mut impl BinWriter) {
        self.0.as_slice().encode_bin(w);
    }

    fn bin_size(&self) -> usize { self.0[0].bin_size() }
}

impl BinDecoder for Witnesses {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let size = u8::decode_bin(r)?;
        if size != 0x01 {
            return Err(BinDecodeError::InvalidLength("Witnesses", "Witness", size as usize));
        }

        Ok(Self([BinDecoder::decode_bin(r)?]))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum WitnessScope {
    None = 0x00,
    CalledByEntry = 0x01,
    CustomContracts = 0x10,
    CustomGroups = 0x20,
    WitnessRules = 0x40,
    Global = 0x80,
}


#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, BinEncode, BinDecode)]
pub struct WitnessScopes {
    scopes: u8,
}

impl WitnessScopes {
    pub fn add_scope(&mut self, scope: WitnessScope) {
        self.scopes |= scope as u8;
    }

    pub fn has_scope(&self, scope: WitnessScope) -> bool {
        self.scopes & (scope as u8) != 0
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, BinEncode, BinDecode)]
pub struct WitnessRule {
    pub action: Action,
    pub condition: WitnessCondition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, BinEncode, BinDecode)]
#[bin(repr = u8)]
pub enum Action {
    Deny = 0x00,
    Allow = 0x01,
}

#[derive(Debug, Clone, PartialEq, Eq, BinEncode, BinDecode)]
#[bin(repr = u8)]
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, BinEncode, BinDecode)]
#[bin(repr = u8)]
#[serde(tag = "type")]
pub enum WitnessCondition {
    #[bin(tag = 0x00)]
    Boolean { expression: bool },

    #[bin(tag = 0x01)]
    Not { hash: String },

    #[bin(tag = 0x02)]
    And { expressions: Vec<WitnessCondition> },

    #[bin(tag = 0x03)]
    Or { expressions: Vec<WitnessCondition> },

    #[bin(tag = 0x18)]
    ScriptHash { hash: String },

    #[bin(tag = 0x19)]
    Group { group: String },

    #[bin(tag = 0x20)]
    CalledByEntry {},

    #[bin(tag = 0x28)]
    CalledByContract { hash: String },

    #[bin(tag = 0x29)]
    CalledByGroup { group: String },
}

#[cfg(test)]
mod test {
    use crate::tx::*;

    #[test]
    fn test_witness_scope() {
        let cond = WitnessCondition::Boolean { expression: true };

        let cond = serde_json::to_string(&cond)
            .expect("json encode should be ok");
        assert_eq!(&cond, r#"{"type":"Boolean","expression":true}"#);

        let entry = WitnessCondition::CalledByEntry {};
        let entry = serde_json::to_string(&entry)
            .expect("json encode should be ok");
        assert_eq!(&entry, r#"{"type":"CalledByEntry"}"#);

        let rule = WitnessRule {
            action: Action::Allow,
            condition: WitnessCondition::Boolean { expression: true },
        };
        let rule = serde_json::to_string(&rule)
            .expect("json encode should be ok");
        assert_eq!(&rule, r#"{"action":"Allow","condition":{"type":"Boolean","expression":true}}"#);

        let attr = TxAttr::OracleResponse(OracleResponse {
            id: 0,
            code: OracleCode::Success,
            result: Default::default(),
        });
        let attr = serde_json::to_string(&attr)
            .expect("json encode should be ok");
        assert_eq!(&attr, r#"{"type":"OracleResponse","id":0,"code":"Success","result":""}"#);

        let got = serde_json::from_str::<WitnessRule>(rule.as_str())
            .expect("json decode should be ok");
        assert_eq!(got.action, Action::Allow);
        assert_eq!(got.condition, WitnessCondition::Boolean { expression: true });

        let data = r#"{"action":"404","condition":{"type":"Boolean","expression":true}}"#;
        let _ = serde_json::from_str::<WitnessRule>(data)
            .expect_err("json decode should be fail");
    }

    #[test]
    fn test_witnesses() {
        let wit = Witnesses::from(Witness::new(b"hello".as_ref().into(), b"world".as_ref().into()));
        let wit = serde_json::to_string(&wit)
            .expect("json encode should be ok");

        let expected = r#"[{"invocation_script":"aGVsbG8=","verification_script":"d29ybGQ="}]"#;
        assert_eq!(&wit, expected);
    }
}