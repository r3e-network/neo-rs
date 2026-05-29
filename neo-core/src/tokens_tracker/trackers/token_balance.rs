//! Token balance record stored by TokensTracker.
//!
//! Represents a user's balance for a specific token.

use super::super::extensions::bigint_var_size;
use crate::neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

/// Balance record for a token.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TokenBalance {
    /// Current balance amount.
    pub balance: BigInt,
    /// Block height when the balance was last updated.
    pub last_updated_block: u32,
}

impl Serializable for TokenBalance {
    fn size(&self) -> usize {
        bigint_var_size(&self.balance) + 4
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        let bytes = self.balance.to_signed_bytes_le();
        writer.write_var_bytes(&bytes)?;
        writer.write_u32(self.last_updated_block)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        // A VM BigInteger is capped at 256 bits; its signed little-endian form
        // needs at most 33 bytes (32 data + 1 sign). Bound the read so a
        // corrupt/adversarial record cannot trigger an unbounded allocation.
        let bytes = reader.read_var_bytes(33)?;
        let balance = BigInt::from_signed_bytes_le(&bytes);
        let last_updated_block = reader.read_u32()?;
        Ok(Self {
            balance,
            last_updated_block,
        })
    }
}
