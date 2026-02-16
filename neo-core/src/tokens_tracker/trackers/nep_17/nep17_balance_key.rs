//! NEP-17 balance key.
//!
//! Storage key for NEP-17 token balances.

use crate::UInt160;
use crate::neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

/// Key for NEP-17 balance records.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Nep17BalanceKey {
    /// User's script hash.
    pub user_script_hash: UInt160,
    /// Token contract's script hash.
    pub asset_script_hash: UInt160,
}

impl Nep17BalanceKey {
    /// Creates a new balance key.
    pub fn new(user_script_hash: UInt160, asset_script_hash: UInt160) -> Self {
        Self {
            user_script_hash,
            asset_script_hash,
        }
    }
}

crate::impl_ord_by_fields!(Nep17BalanceKey, user_script_hash, asset_script_hash);

impl Serializable for Nep17BalanceKey {
    fn size(&self) -> usize {
        40
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_serializable(&self.user_script_hash)?;
        writer.write_serializable(&self.asset_script_hash)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        Ok(Self {
            user_script_hash: <UInt160 as Serializable>::deserialize(reader)?,
            asset_script_hash: <UInt160 as Serializable>::deserialize(reader)?,
        })
    }
}
