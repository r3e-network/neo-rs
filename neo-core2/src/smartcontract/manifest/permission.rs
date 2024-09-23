use std::cmp::Ordering;
use std::convert::TryFrom;
use elliptic_curve::sec1::ToEncodedPoint;
use p256::PublicKey;
use serde::{Deserialize, Serialize};

use crate::crypto::keys;
use crate::util::Uint160;
use crate::vm::stackitem::{Item, StackItem};

/// Represents permission type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PermissionType {
    /// Allows everything.
    Wildcard = 0,
    /// Restricts called contracts based on hash.
    Hash = 1,
    /// Restricts called contracts based on public key.
    Group = 2,
}

/// Permission descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDesc {
    pub typ: PermissionType,
    pub value: Option<PermissionValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionValue {
    Hash(Uint160),
    Group(PublicKey),
}

/// Describes which contracts may be invoked and which methods are called.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    pub contract: PermissionDesc,
    pub methods: WildStrings,
}

/// Array of Permission.
pub type Permissions = Vec<Permission>;

impl PermissionDesc {
    pub fn new(typ: PermissionType, value: Option<PermissionValue>) -> Self {
        match (typ, &value) {
            (PermissionType::Wildcard, None) => PermissionDesc { typ, value },
            (PermissionType::Hash, Some(PermissionValue::Hash(_))) => PermissionDesc { typ, value },
            (PermissionType::Group, Some(PermissionValue::Group(_))) => PermissionDesc { typ, value },
            _ => panic!("Invalid permission descriptor"),
        }
    }

    pub fn hash(&self) -> Option<&Uint160> {
        if let Some(PermissionValue::Hash(hash)) = &self.value {
            Some(hash)
        } else {
            None
        }
    }

    pub fn group(&self) -> Option<&PublicKey> {
        if let Some(PermissionValue::Group(key)) = &self.value {
            Some(key)
        } else {
            None
        }
    }

    pub fn compare(&self, other: &PermissionDesc) -> Ordering {
        match self.typ.cmp(&other.typ) {
            Ordering::Equal => match (&self.value, &other.value) {
                (Some(PermissionValue::Hash(a)), Some(PermissionValue::Hash(b))) => a.cmp(b),
                (Some(PermissionValue::Group(a)), Some(PermissionValue::Group(b))) => {
                    a.to_encoded_point(false).as_bytes().cmp(b.to_encoded_point(false).as_bytes())
                }
                _ => Ordering::Equal,
            },
            ord => ord,
        }
    }
}

impl Permission {
    pub fn new(typ: PermissionType, value: Option<PermissionValue>) -> Self {
        Permission {
            contract: PermissionDesc::new(typ, value),
            methods: WildStrings::default(),
        }
    }

    pub fn is_valid(&self) -> Result<(), &'static str> {
        if self.methods.value.as_ref().map_or(false, |v| v.contains(&String::new())) {
            return Err("empty method name");
        }
        if self.methods.value.as_ref().map_or(false, |v| has_duplicates(v)) {
            return Err("duplicate method names");
        }
        Ok(())
    }

    pub fn is_allowed(&self, hash: &Uint160, m: &Manifest, method: &str) -> bool {
        match self.contract.typ {
            PermissionType::Wildcard => {}
            PermissionType::Hash => {
                if self.contract.hash() != Some(hash) {
                    return false;
                }
            }
            PermissionType::Group => {
                let contract_group = self.contract.group().unwrap();
                if !m.groups.iter().any(|group| group.public_key == *contract_group) {
                    return false;
                }
            }
        }
        self.methods.is_wildcard() || self.methods.contains(method)
    }
}

impl Permissions {
    pub fn are_valid(&self) -> Result<(), &'static str> {
        for permission in self {
            permission.is_valid()?;
        }
        if has_duplicates(self) {
            return Err("contracts have duplicates");
        }
        Ok(())
    }
}

// Implement necessary traits and methods for WildStrings, Manifest, and other types as needed.

fn has_duplicates<T: Ord>(slice: &[T]) -> bool {
    let mut sorted = slice.to_vec();
    sorted.sort();
    sorted.windows(2).any(|w| w[0] == w[1])
}

// Implement serialization, deserialization, and conversion methods as needed.
