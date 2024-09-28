use std::error::Error;
use num_bigint::BigInt;
use crate::crypto::keys::PublicKey;
use crate::vm::stackitem::{Item, Convertible};

// IDList is a list of oracle request IDs.
pub type IDList = Vec<u64>;

// NodeList represents a list of oracle nodes.
pub type NodeList = Vec<PublicKey>;

impl Convertible for IDList {
    // ToStackItem implements stackitem::Convertible. It never returns an error.
    fn to_stack_item(&self) -> Result<Item, Box<dyn Error>> {
        let arr: Vec<Item> = self.iter()
            .map(|&id| Item::BigInteger(BigInt::from(id)))
            .collect();
        Ok(Item::Array(arr))
    }

    // FromStackItem implements stackitem::Convertible.
    fn from_stack_item(item: &Item) -> Result<Self, Box<dyn Error>> {
        if let Item::Array(arr) = item {
            let mut result = IDList::with_capacity(arr.len());
            for item in arr {
                if let Item::BigInteger(bi) = item {
                    result.push(bi.to_u64().ok_or("Integer overflow")?);
                } else {
                    return Err("Expected BigInteger item".into());
                }
            }
            Ok(result)
        } else {
            Err("Expected Array item".into())
        }
    }
}

impl IDList {
    // Remove removes id from the list.
    pub fn remove(&mut self, id: u64) -> bool {
        if let Some(index) = self.iter().position(|&x| x == id) {
            self.remove(index);
            true
        } else {
            false
        }
    }
}

impl Convertible for NodeList {
    // ToStackItem implements stackitem::Convertible. It never returns an error.
    fn to_stack_item(&self) -> Result<Item, Box<dyn Error>> {
        let arr: Vec<Item> = self.iter()
            .map(|key| Item::ByteArray(key.to_bytes()))
            .collect();
        Ok(Item::Array(arr))
    }

    // FromStackItem implements stackitem::Convertible.
    fn from_stack_item(item: &Item) -> Result<Self, Box<dyn Error>> {
        if let Item::Array(arr) = item {
            let mut result = NodeList::with_capacity(arr.len());
            for item in arr {
                if let Item::ByteArray(bytes) = item {
                    let pub_key = PublicKey::from_bytes(bytes)?;
                    result.push(pub_key);
                } else {
                    return Err("Expected ByteArray item".into());
                }
            }
            Ok(result)
        } else {
            Err("Expected Array item".into())
        }
    }
}
