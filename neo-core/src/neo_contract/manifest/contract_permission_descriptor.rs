use NeoRust::prelude::Secp256r1PublicKey;
use neo_vm::stack_item::StackItem;
use crate::uint160::UInt160;

/// Indicates which contracts are authorized to be called.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractPermissionDescriptor {
    /// The hash of the contract. It can't be set with `group`.
    pub hash: Option<UInt160>,
    /// The group of the contracts. It can't be set with `hash`.
    pub group: Option<Secp256r1PublicKey>,
}

impl ContractPermissionDescriptor {
    /// Creates a new instance with the specified contract hash.
    pub fn new_with_hash(hash: UInt160) -> Self {
        Self {
            hash: Some(hash),
            group: None,
        }
    }

    /// Creates a new instance with the specified group.
    pub fn new_with_group(group: Secp256r1PublicKey) -> Self {
        Self {
            hash: None,
            group: Some(group),
        }
    }

    /// Creates a new instance with wildcard.
    pub fn new_wildcard() -> Self {
        Self {
            hash: None,
            group: None,
        }
    }

    /// Indicates whether `hash` is set.
    pub fn is_hash(&self) -> bool {
        self.hash.is_some()
    }

    /// Indicates whether `group` is set.
    pub fn is_group(&self) -> bool {
        self.group.is_some()
    }

    /// Indicates whether it is a wildcard.
    pub fn is_wildcard(&self) -> bool {
        self.hash.is_none() && self.group.is_none()
    }

    /// Creates a new instance from a StackItem.
    pub fn from_stack_item(item: &StackItem) -> Result<Self, Error> {
        if item.is_null() {
            Ok(Self::new_wildcard())
        } else {
            let span = item.get_span()?;
            Self::from_bytes(&span)
        }
    }

    /// Creates a new instance from bytes.
    pub fn from_bytes(span: &[u8]) -> Result<Self, Error> {
        match span.len() {
            20 => Ok(Self::new_with_hash(UInt160::from_slice(span)?)),
            33 => Ok(Self::new_with_group(Secp256r1PublicKey::decode_point(span, ECCurve::Secp256r1)?)),
            _ => Err(Error::ArgumentError("Invalid byte length".into())),
        }
    }

    /// Converts the permission descriptor from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, Error> {
        match json.len() {
            42 => Ok(Self::new_with_hash(UInt160::from_str(json)?)),
            66 => Ok(Self::new_with_group(Secp256r1PublicKey::from_str(json)?)),
            1 if json == "*" => Ok(Self::new_wildcard()),
            _ => Err(Error::FormatError("Invalid JSON format".into())),
        }
    }

    /// Converts the permission descriptor to a JSON string.
    pub fn to_json(&self) -> String {
        if let Some(hash) = &self.hash {
            hash.to_string()
        } else if let Some(group) = &self.group {
            group.to_string()
        } else {
            "*".to_string()
        }
    }

    /// Converts the permission descriptor to byte array.
    pub fn to_array(&self) -> Option<Vec<u8>> {
        if let Some(hash) = &self.hash {
            Some(hash.to_vec())
        } else if let Some(group) = &self.group {
            Some(group.encode_point(true))
        } else {
            None
        }
    }
}

impl PartialEq for ContractPermissionDescriptor {
    fn eq(&self, other: &Self) -> bool {
        if self.is_wildcard() && other.is_wildcard() {
            return true;
        }
        if self.is_hash() && other.is_hash() {
            return self.hash == other.hash;
        }
        if self.is_group() && other.is_group() {
            return self.group == other.group;
        }
        false
    }
}

impl Eq for ContractPermissionDescriptor {}

impl std::hash::Hash for ContractPermissionDescriptor {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
        self.group.hash(state);
    }
}
