use alloc::{vec, vec::Vec};

use crate::tx::{Action, WitnessCondition, WitnessRule, WitnessScope};

use super::super::Signer;
use super::utils::h160_to_hash160;

impl Signer {
    pub fn all_rules(&self) -> Vec<WitnessRule> {
        if self.scopes.has_scope(WitnessScope::Global) {
            return vec![WitnessRule::new(
                Action::Allow,
                WitnessCondition::Boolean { expression: true },
            )];
        }

        let mut rules = Vec::new();
        if self.scopes.has_scope(WitnessScope::CalledByEntry) {
            rules.push(WitnessRule::new(
                Action::Allow,
                WitnessCondition::CalledByEntry,
            ));
        }
        if self.scopes.has_scope(WitnessScope::CustomContracts) {
            for hash in &self.allowed_contract {
                rules.push(WitnessRule::new(
                    Action::Allow,
                    WitnessCondition::ScriptHash {
                        hash: h160_to_hash160(hash),
                    },
                ));
            }
        }
        if self.scopes.has_scope(WitnessScope::CustomGroups) {
            for group in &self.allowed_groups {
                rules.push(WitnessRule::new(
                    Action::Allow,
                    WitnessCondition::Group {
                        group: group.clone(),
                    },
                ));
            }
        }
        if self.scopes.has_scope(WitnessScope::WitnessRules) {
            rules.extend(self.rules.iter().cloned());
        }
        rules
    }
}
