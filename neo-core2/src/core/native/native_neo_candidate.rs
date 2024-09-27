use std::panic;
use num_bigint::BigInt;
use crate::vm::stackitem::{Item, Struct, Convertible};

pub struct Candidate {
    registered: bool,
    votes: BigInt,
}

impl Candidate {
    // FromBytes unmarshals a candidate from the byte array.
    pub fn from_bytes(data: &[u8]) -> Self {
        let mut candidate = Self {
            registered: false,
            votes: BigInt::from(0),
        };
        if let Err(e) = Convertible::from_bytes(data, &mut candidate) {
            panic!("{}", e);
        }
        candidate
    }

    // ToStackItem implements stackitem::Convertible. It never returns an error.
    pub fn to_stack_item(&self) -> Item {
        Item::Struct(Struct::new(vec![
            Item::Boolean(self.registered),
            Item::Integer(self.votes.clone()),
        ]))
    }

    // FromStackItem implements stackitem::Convertible.
    pub fn from_stack_item(&mut self, item: &Item) -> Result<(), Box<dyn std::error::Error>> {
        if let Item::Struct(s) = item {
            let arr = s.value();
            if arr.len() != 2 {
                return Err("Invalid number of items in struct".into());
            }
            self.registered = arr[0].try_bool()?;
            self.votes = arr[1].try_integer()?;
            Ok(())
        } else {
            Err("Expected Struct item".into())
        }
    }
}

impl Convertible for Candidate {
    fn from_bytes(data: &[u8], target: &mut Self) -> Result<(), Box<dyn std::error::Error>> {
        // Implement deserialization logic here
        unimplemented!()
    }

    fn to_bytes(&self) -> Vec<u8> {
        // Implement serialization logic here
        unimplemented!()
    }
}
