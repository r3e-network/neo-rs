use neo_crypto::ecc::Secp256r1PublicKey;
use neo_io::{BinaryReader, BinaryWriter, Serializable};
use neo_json::json::{JObject, JArray};
use neo_network::p2p::payloads::conditions::{WitnessRule, WitnessRuleAction, BooleanCondition, CalledByEntryCondition, ScriptHashCondition, GroupCondition};
use neo_smart_contract::UInt160;
use neo_vm::types::{StackItem, Array, ByteString};
use std::collections::HashSet;
use std::convert::TryFrom;
use std::io::{self, Read, Write};
use NeoRust::crypto::Secp256r1PublicKey;
use crate::network::Payloads::WitnessRule;
use crate::uint160::UInt160;

const MAX_SUBITEMS: usize = 16;

#[derive(Clone, Debug)]
pub struct Signer {
    pub account: UInt160,
    pub scopes: WitnessScope,
    pub allowed_contracts: Vec<UInt160>,
    pub allowed_groups: Vec<Secp256r1PublicKey>,
    pub rules: Vec<WitnessRule>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WitnessScope {
    None = 0,
    CalledByEntry = 0x01,
    CustomContracts = 0x02,
    CustomGroups = 0x04,
    WitnessRules = 0x08,
    Global = 0x10,
}

impl Signer {
    pub fn size(&self) -> usize {
        UInt160::len() +
        std::mem::size_of::<WitnessScope>() +
        if self.scopes.contains(WitnessScope::CustomContracts) { self.allowed_contracts.len() * UInt160::len() } else { 0 } +
        if self.scopes.contains(WitnessScope::CustomGroups) { self.allowed_groups.len() * Secp256r1PublicKey::len() } else { 0 } +
        if self.scopes.contains(WitnessScope::WitnessRules) { self.rules.iter().map(|r| r.size()).sum::<usize>() } else { 0 }
    }

    pub fn deserialize<R: Read>(reader: &mut R) -> io::Result<Self> {
        let account = UInt160::deserialize(reader)?;
        let scopes = WitnessScope::try_from(reader.read_u8()?)?;

        if (scopes as u8 & !(WitnessScope::CalledByEntry as u8 | WitnessScope::CustomContracts as u8 | WitnessScope::CustomGroups as u8 | WitnessScope::WitnessRules as u8 | WitnessScope::Global as u8)) != 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid witness scope"));
        }

        if scopes.contains(WitnessScope::Global) && scopes != WitnessScope::Global {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid witness scope"));
        }

        let allowed_contracts = if scopes.contains(WitnessScope::CustomContracts) {
            let mut contracts = Vec::new();
            for _ in 0..MAX_SUBITEMS {
                contracts.push(UInt160::deserialize(reader)?);
            }
            contracts
        } else {
            Vec::new()
        };

        let allowed_groups = if scopes.contains(WitnessScope::CustomGroups) {
            let mut groups = Vec::new();
            for _ in 0..MAX_SUBITEMS {
                groups.push(Secp256r1PublicKey::deserialize(reader)?);
            }
            groups
        } else {
            Vec::new()
        };

        let rules = if scopes.contains(WitnessScope::WitnessRules) {
            let mut rules = Vec::new();
            for _ in 0..MAX_SUBITEMS {
                rules.push(WitnessRule::deserialize(reader)?);
            }
            rules
        } else {
            Vec::new()
        };

        Ok(Self {
            account,
            scopes,
            allowed_contracts,
            allowed_groups,
            rules,
        })
    }

    pub fn get_all_rules(&self) -> Vec<WitnessRule> {
        let mut rules = Vec::new();

        if self.scopes == WitnessScope::Global {
            rules.push(WitnessRule {
                action: WitnessRuleAction::Allow,
                condition: Box::new(BooleanCondition { expression: true }),
            });
        } else {
            if self.scopes.contains(WitnessScope::CalledByEntry) {
                rules.push(WitnessRule {
                    action: WitnessRuleAction::Allow,
                    condition: Box::new(CalledByEntryCondition {}),
                });
            }
            if self.scopes.contains(WitnessScope::CustomContracts) {
                for hash in &self.allowed_contracts {
                    rules.push(WitnessRule {
                        action: WitnessRuleAction::Allow,
                        condition: Box::new(ScriptHashCondition { hash: *hash }),
                    });
                }
            }
            if self.scopes.contains(WitnessScope::CustomGroups) {
                for group in &self.allowed_groups {
                    rules.push(WitnessRule {
                        action: WitnessRuleAction::Allow,
                        condition: Box::new(GroupCondition { group: *group }),
                    });
                }
            }
            if self.scopes.contains(WitnessScope::WitnessRules) {
                rules.extend_from_slice(&self.rules);
            }
        }

        rules
    }

    pub fn serialize<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        self.account.serialize(writer)?;
        writer.write_u8(self.scopes as u8)?;
        if self.scopes.contains(WitnessScope::CustomContracts) {
            for contract in &self.allowed_contracts {
                contract.serialize(writer)?;
            }
        }
        if self.scopes.contains(WitnessScope::CustomGroups) {
            for group in &self.allowed_groups {
                group.serialize(writer)?;
            }
        }
        if self.scopes.contains(WitnessScope::WitnessRules) {
            for rule in &self.rules {
                rule.serialize(writer)?;
            }
        }
        Ok(())
    }

    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let account = UInt160::from_str(json.get("account").ok_or("Missing account")?.as_str().ok_or("Invalid account")?)?;
        let scopes = WitnessScope::try_from(json.get("scopes").ok_or("Missing scopes")?.as_u64().ok_or("Invalid scopes")? as u8)?;

        let allowed_contracts = if scopes.contains(WitnessScope::CustomContracts) {
            json.get("allowedcontracts").ok_or("Missing allowedcontracts")?
                .as_array().ok_or("Invalid allowedcontracts")?
                .iter()
                .map(|p| UInt160::from_str(p.as_str().ok_or("Invalid contract")?))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };

        let allowed_groups = if scopes.contains(WitnessScope::CustomGroups) {
            json.get("allowedgroups").ok_or("Missing allowedgroups")?
                .as_array().ok_or("Invalid allowedgroups")?
                .iter()
                .map(|p| Secp256r1PublicKey::from_str(p.as_str().ok_or("Invalid group")?))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };

        let rules = if scopes.contains(WitnessScope::WitnessRules) {
            json.get("rules").ok_or("Missing rules")?
                .as_array().ok_or("Invalid rules")?
                .iter()
                .map(|p| WitnessRule::from_json(p.as_object().ok_or("Invalid rule")?))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };

        Ok(Self {
            account,
            scopes,
            allowed_contracts,
            allowed_groups,
            rules,
        })
    }

    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("account", self.account.to_string().into());
        json.insert("scopes", (self.scopes as u8).into());
        if self.scopes.contains(WitnessScope::CustomContracts) {
            json.insert("allowedcontracts", JArray::from(self.allowed_contracts.iter().map(|p| p.to_string().into()).collect::<Vec<_>>()));
        }
        if self.scopes.contains(WitnessScope::CustomGroups) {
            json.insert("allowedgroups", JArray::from(self.allowed_groups.iter().map(|p| p.to_string().into()).collect::<Vec<_>>()));
        }
        if self.scopes.contains(WitnessScope::WitnessRules) {
            json.insert("rules", JArray::from(self.rules.iter().map(|p| p.to_json()).collect::<Vec<_>>()));
        }
        json
    }

    pub fn to_stack_item(&self) -> StackItem {
        Array::new(vec![
            ByteString::new(self.account.to_vec()),
            (self.scopes as u8).into(),
            if self.scopes.contains(WitnessScope::CustomContracts) {
                Array::new(self.allowed_contracts.iter().map(|u| ByteString::new(u.to_vec())).collect())
            } else {
                Array::new(vec![])
            },
            if self.scopes.contains(WitnessScope::CustomGroups) {
                Array::new(self.allowed_groups.iter().map(|u| ByteString::new(u.to_vec())).collect())
            } else {
                Array::new(vec![])
            },
            if self.scopes.contains(WitnessScope::WitnessRules) {
                Array::new(self.rules.iter().map(|u| u.to_stack_item()).collect())
            } else {
                Array::new(vec![])
            },
        ])
    }
}
