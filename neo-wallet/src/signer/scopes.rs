use alloc::vec::Vec;

use bitflags::bitflags;

bitflags! {
    /// Witness scopes describing how a signer may be used during verification.
    #[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct SignerScopes: u8 {
        const NONE = 0x00;
        const CALLED_BY_ENTRY = 0x01;
        const CUSTOM_CONTRACTS = 0x02;
        const CUSTOM_GROUPS = 0x04;
        const WITNESS_RULES = 0x08;
        const GLOBAL = 0x10;
    }
}

impl SignerScopes {
    pub const fn global() -> Self {
        Self::GLOBAL
    }

    pub const fn called_by_entry() -> Self {
        Self::CALLED_BY_ENTRY
    }

    pub fn is_valid(self) -> bool {
        if self.is_empty() {
            return true;
        }
        if self.contains(Self::GLOBAL) {
            return self == Self::GLOBAL;
        }
        true
    }

    pub fn to_witness_scope_string(self) -> String {
        if self.contains(Self::GLOBAL) {
            return "Global".to_string();
        }
        if self.is_empty() {
            return "None".to_string();
        }
        let mut parts = Vec::new();
        if self.contains(Self::CALLED_BY_ENTRY) {
            parts.push("CalledByEntry");
        }
        if self.contains(Self::CUSTOM_CONTRACTS) {
            parts.push("CustomContracts");
        }
        if self.contains(Self::CUSTOM_GROUPS) {
            parts.push("CustomGroups");
        }
        if self.contains(Self::WITNESS_RULES) {
            parts.push("WitnessRules");
        }
        if parts.is_empty() {
            parts.push("None");
        }
        parts.join("|")
    }

    pub fn from_witness_scope_string(value: &str) -> Option<Self> {
        let mut scopes = SignerScopes::NONE;
        for part in value.split(|c| c == '|' || c == ',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            match part {
                "None" => {}
                "CalledByEntry" => scopes |= SignerScopes::CALLED_BY_ENTRY,
                "CustomContracts" => scopes |= SignerScopes::CUSTOM_CONTRACTS,
                "CustomGroups" => scopes |= SignerScopes::CUSTOM_GROUPS,
                "WitnessRules" => scopes |= SignerScopes::WITNESS_RULES,
                "Global" => return Some(SignerScopes::GLOBAL),
                _ => return None,
            }
        }
        if scopes.is_empty() {
            Some(SignerScopes::NONE)
        } else {
            Some(scopes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_global_scope() {
        assert!(SignerScopes::GLOBAL.is_valid());
        let invalid = SignerScopes::GLOBAL | SignerScopes::CALLED_BY_ENTRY;
        assert!(!invalid.is_valid());
    }
}
