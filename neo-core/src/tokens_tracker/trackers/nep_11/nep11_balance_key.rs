//! NEP-11 balance key.
//!
//! Storage key for NEP-11 (NFT) balances.

use super::super::super::extensions::bytes_var_size;
use crate::UInt160;
use crate::neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use neo_vm::stack_item::ByteString;
use num_bigint::BigInt;
use num_traits::Zero;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// Storage key for NEP-11 balances: `[UserScriptHash, AssetScriptHash, TokenId]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Nep11BalanceKey {
    /// User's script hash.
    pub user_script_hash: UInt160,
    /// NFT contract's script hash.
    pub asset_script_hash: UInt160,
    /// Token ID.
    pub token: Vec<u8>,
}

impl Nep11BalanceKey {
    /// Creates a new balance key.
    pub fn new(user_script_hash: UInt160, asset_script_hash: UInt160, token_id: Vec<u8>) -> Self {
        Self {
            user_script_hash,
            asset_script_hash,
            token: token_id,
        }
    }

    fn token_integer(&self) -> BigInt {
        ByteString::new(self.token.clone())
            .to_integer()
            .unwrap_or_else(|_| BigInt::zero())
    }
}

impl PartialOrd for Nep11BalanceKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Nep11BalanceKey {
    fn cmp(&self, other: &Self) -> Ordering {
        let user_cmp = self.user_script_hash.cmp(&other.user_script_hash);
        if user_cmp != Ordering::Equal {
            return user_cmp;
        }
        let asset_cmp = self.asset_script_hash.cmp(&other.asset_script_hash);
        if asset_cmp != Ordering::Equal {
            return asset_cmp;
        }
        self.token_integer().cmp(&other.token_integer())
    }
}

impl Serializable for Nep11BalanceKey {
    fn size(&self) -> usize {
        20 + 20 + bytes_var_size(self.token.len())
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_serializable(&self.user_script_hash)?;
        writer.write_serializable(&self.asset_script_hash)?;
        writer.write_var_bytes(&self.token)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let user_script_hash = <UInt160 as Serializable>::deserialize(reader)?;
        let asset_script_hash = <UInt160 as Serializable>::deserialize(reader)?;
        let token = reader.read_var_bytes(usize::MAX)?;
        Ok(Self {
            user_script_hash,
            asset_script_hash,
            token,
        })
    }
}
