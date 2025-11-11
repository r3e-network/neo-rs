use alloc::{string::ToString, vec::Vec};
use core::fmt;

use super::super::{WitnessScope, WitnessScopes};

impl fmt::Display for WitnessScopes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.scopes == WitnessScope::None as u8 {
            return write!(f, "None");
        }
        if self.scopes == WitnessScope::Global as u8 {
            return write!(f, "Global");
        }

        let mut parts = Vec::new();
        if self.has_scope(WitnessScope::CalledByEntry) {
            parts.push(WitnessScope::CalledByEntry.to_string());
        }
        if self.has_scope(WitnessScope::CustomContracts) {
            parts.push(WitnessScope::CustomContracts.to_string());
        }
        if self.has_scope(WitnessScope::CustomGroups) {
            parts.push(WitnessScope::CustomGroups.to_string());
        }
        if self.has_scope(WitnessScope::WitnessRules) {
            parts.push(WitnessScope::WitnessRules.to_string());
        }

        write!(f, "{}", parts.join("|"))
    }
}
