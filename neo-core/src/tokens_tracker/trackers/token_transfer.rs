//! Token transfer record stored by TokensTracker.
//!
//! Represents a single token transfer event.

use super::super::extensions::bigint_var_size;
use crate::neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use crate::{UInt160, UInt256};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

/// Transfer record for tracking token movements.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TokenTransfer {
    /// The other party in the transfer (sender for received, receiver for sent).
    pub user_script_hash: UInt160,
    /// Block index where the transfer occurred.
    pub block_index: u32,
    /// Transaction hash containing the transfer.
    pub tx_hash: UInt256,
    /// Amount transferred.
    pub amount: BigInt,
}

impl Serializable for TokenTransfer {
    fn size(&self) -> usize {
        20 + 4 + 32 + bigint_var_size(&self.amount)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_serializable(&self.user_script_hash)?;
        writer.write_u32(self.block_index)?;
        writer.write_serializable(&self.tx_hash)?;
        let bytes = self.amount.to_signed_bytes_le();
        writer.write_var_bytes(&bytes)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let user_script_hash = <UInt160 as Serializable>::deserialize(reader)?;
        let block_index = reader.read_u32()?;
        let tx_hash = <UInt256 as Serializable>::deserialize(reader)?;
        let amount_bytes = reader.read_var_bytes(32)?;
        let amount = BigInt::from_signed_bytes_le(&amount_bytes);
        Ok(Self {
            user_script_hash,
            block_index,
            tx_hash,
            amount,
        })
    }
}
