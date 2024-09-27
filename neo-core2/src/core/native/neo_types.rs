use crypto::elliptic;
use std::error::Error;
use num_bigint::BigInt;

use crate::crypto::keys;
use crate::vm::stackitem;

// gasIndexPair contains the block index together with the generated gas per block.
// It is used to cache NEO GASRecords.
struct GasIndexPair {
    index: u32,
    gas_per_block: BigInt,
}

// gasRecord contains the history of gas per block changes. It is used only by NEO cache.
type GasRecord = Vec<GasIndexPair>;

// keyWithVotes is a serialized key with votes balance. It's not deserialized
// because some uses of it imply serialized-only usage and converting to
// PublicKey is quite expensive.
struct KeyWithVotes {
    key: String,
    votes: BigInt,
    // UnmarshaledKey contains public key if it was unmarshaled.
    unmarshaled_key: Option<keys::PublicKey>,
}

type KeysWithVotes = Vec<KeyWithVotes>;

impl KeyWithVotes {
    // PublicKey unmarshals and returns the public key of k.
    fn public_key(&self) -> Result<keys::PublicKey, Box<dyn Error>> {
        if let Some(ref key) = self.unmarshaled_key {
            Ok(key.clone())
        } else {
            keys::new_public_key_from_bytes(self.key.as_bytes(), elliptic::P256::new())
        }
    }
}

impl KeysWithVotes {
    fn to_stack_item(&self) -> stackitem::Item {
        let arr: Vec<stackitem::Item> = self.iter().map(|k| {
            stackitem::Item::Struct(vec![
                stackitem::Item::ByteArray(k.key.as_bytes().to_vec()),
                stackitem::Item::BigInteger(k.votes.clone()),
            ])
        }).collect();
        stackitem::Item::Array(arr)
    }

    // toNotificationItem converts keysWithVotes to a stackitem::Item suitable for use in a notification,
    // including public keys only.
    fn to_notification_item(&self) -> stackitem::Item {
        let arr: Vec<stackitem::Item> = self.iter().map(|k| {
            stackitem::Item::ByteArray(k.key.as_bytes().to_vec())
        }).collect();
        stackitem::Item::Array(arr)
    }

    fn from_stack_item(&mut self, item: &stackitem::Item) -> Result<(), Box<dyn Error>> {
        if let stackitem::Item::Array(arr) = item {
            let mut kvs = Vec::with_capacity(arr.len());
            for item in arr {
                if let stackitem::Item::Struct(s) = item {
                    if s.len() < 2 {
                        return Err("invalid length".into());
                    }
                    let pub_key = s[0].try_bytes()?;
                    let votes = s[1].try_integer()?;
                    kvs.push(KeyWithVotes {
                        key: String::from_utf8(pub_key)?,
                        votes,
                        unmarshaled_key: None,
                    });
                } else {
                    return Err("element is not a struct".into());
                }
            }
            *self = kvs;
            Ok(())
        } else {
            Err("not an array".into())
        }
    }

    // Bytes serializes keys with votes slice.
    fn bytes(&self, sc: &stackitem::SerializationContext) -> Vec<u8> {
        sc.serialize(&self.to_stack_item(), false).expect("Serialization failed")
    }

    // DecodeBytes deserializes keys and votes slice.
    fn decode_bytes(&mut self, data: &[u8]) -> Result<(), Box<dyn Error>> {
        let item = stackitem::deserialize(data)?;
        self.from_stack_item(&item)
    }
}
