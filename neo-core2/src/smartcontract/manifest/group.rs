use std::error::Error;
use std::cmp::Ordering;
use hex;
use serde::{Serialize, Deserialize};
use neo_crypto::{keys::PublicKey, hash};
use neo_types::Uint160;
use neo_vm::stackitem::{Item, StructItem};

/// Group represents a group of smartcontracts identified by a public key.
/// Every SC in a group must provide signature of its hash to prove
/// it belongs to the group.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    #[serde(rename = "pubkey")]
    pub public_key: PublicKey,
    pub signature: Vec<u8>,
}

/// Groups is just a vector of Group.
pub type Groups = Vec<Group>;

impl Group {
    /// Checks whether the group's signature corresponds to the given hash.
    pub fn is_valid(&self, h: &Uint160) -> Result<(), Box<dyn Error>> {
        if !self.public_key.verify(&self.signature, &hash::sha256(&h.to_be_bytes())) {
            return Err("incorrect group signature".into());
        }
        Ok(())
    }

    /// Converts Group to Item.
    pub fn to_stack_item(&self) -> Item {
        Item::Struct(StructItem::new(vec![
            Item::ByteArray(self.public_key.to_bytes()),
            Item::ByteArray(self.signature.clone()),
        ]))
    }

    /// Converts Item to Group.
    pub fn from_stack_item(item: &Item) -> Result<Self, Box<dyn Error>> {
        if let Item::Struct(struct_item) = item {
            let items = struct_item.value();
            if items.len() != 2 {
                return Err("invalid Group stackitem length".into());
            }
            let p_key = items[0].try_bytes()?;
            let public_key = PublicKey::from_bytes(&p_key)?;
            let signature = items[1].try_bytes()?;
            if signature.len() != PublicKey::SIGNATURE_LEN {
                return Err("wrong signature length".into());
            }
            Ok(Group { public_key, signature })
        } else {
            Err("invalid Group stackitem type".into())
        }
    }
}

impl Groups {
    /// Checks for groups correctness and uniqueness.
    /// If the contract hash is empty, then hash-related checks are omitted.
    pub fn are_valid(&self, h: &Uint160) -> Result<(), Box<dyn Error>> {
        if self.is_empty() {
            return Err("null groups".into());
        }
        if !h.is_zero() {
            for group in self {
                group.is_valid(h)?;
            }
        }
        if self.len() < 2 {
            return Ok(());
        }
        let mut p_keys: Vec<&PublicKey> = self.iter().map(|g| &g.public_key).collect();
        p_keys.sort_by(|a, b| a.cmp(b));
        for i in 1..p_keys.len() {
            if p_keys[i].cmp(p_keys[i-1]) == Ordering::Equal {
                return Err("duplicate group keys".into());
            }
        }
        Ok(())
    }

    /// Checks if the Groups contains a specific PublicKey.
    pub fn contains(&self, k: &PublicKey) -> bool {
        self.iter().any(|gr| k == &gr.public_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Add tests here
}
