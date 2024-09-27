use std::convert::TryFrom;
use std::io::{self, Read, Write};
use crate::cryptography::ECPoint;
use crate::io::iserializable::ISerializable;
use crate::neo_contract::iinteroperable::IInteroperable;
use crate::network::payloads::{WitnessRule, WitnessRuleAction};
use crate::network::payloads::conditions::{BooleanCondition, ScriptHashCondition};
use crate::tx::WitnessScope;
use neo_type::H160;

const MAX_SUBITEMS: usize = 16;

#[derive(Clone, Debug)]
pub struct Signer {
    pub account: H160,
    pub scopes: WitnessScope,
    pub allowed_contracts: Vec<H160>,
    pub allowed_groups: Vec<ECPoint>,
    pub rules: Vec<WitnessRule>,
}

impl Signer {
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

}

impl IInteroperable for Signer {
    type Error;

    fn from_stack_item(stack_item: &Rc<StackItem>) -> Result<Self, Self::Error> {
        todo!()
    }

    fn to_stack_item(&self, reference_counter: Option<&References>) -> Result<Rc<StackItem>, Self::Error> {

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


impl ISerializable for Signer {
     fn size(&self) -> usize {
        H160::len() +
        std::mem::size_of::<WitnessScope>() +
        if self.scopes.contains(WitnessScope::CustomContracts) { self.allowed_contracts.len() * H160::len() } else { 0 } +
        if self.scopes.contains(WitnessScope::CustomGroups) { self.allowed_groups.len() * ECPoint::len() } else { 0 } +
        if self.scopes.contains(WitnessScope::WitnessRules) { self.rules.iter().map(|r| r.size()).sum::<usize>() } else { 0 }
    }

     fn serialize<W: Write>(&self, writer: &mut W) -> io::Result<()> {
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

     fn deserialize(reader: &mut MemoryReader) -> io::Result<Self, std::io::Error> {
        let account = H160::deserialize(reader)?;
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
                contracts.push(H160::deserialize(reader)?);
            }
            contracts
        } else {
            Vec::new()
        };

        let allowed_groups = if scopes.contains(WitnessScope::CustomGroups) {
            let mut groups = Vec::new();
            for _ in 0..MAX_SUBITEMS {
                groups.push(ECPoint::deserialize(reader)?);
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

    
}

impl IJsonConvertible for Signer {
     fn from_json(json: &JObject) -> Result<Self, String> {
        let account = H160::from_str(json.get("account").ok_or("Missing account")?.as_str().ok_or("Invalid account")?)?;
        let scopes = WitnessScope::try_from(json.get("scopes").ok_or("Missing scopes")?.as_u64().ok_or("Invalid scopes")? as u8)?;

        let allowed_contracts = if scopes.contains(WitnessScope::CustomContracts) {
            json.get("allowedcontracts").ok_or("Missing allowedcontracts")?
                .as_array().ok_or("Invalid allowedcontracts")?
                .iter()
                .map(|p| H160::from_str(p.as_str().ok_or("Invalid contract")?))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };

        let allowed_groups = if scopes.contains(WitnessScope::CustomGroups) {
            json.get("allowedgroups").ok_or("Missing allowedgroups")?
                .as_array().ok_or("Invalid allowedgroups")?
                .iter()
                .map(|p| ECPoint::from_str(p.as_str().ok_or("Invalid group")?))
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

     fn to_json(&self) -> JObject {
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
}