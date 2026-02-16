// Copyright (C) 2015-2025 The Neo Project.
//
// conflicts.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::UInt256;
use crate::neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::LedgerContract;
use serde::{Deserialize, Serialize};
use tracing::warn;

/// Represents a conflicts transaction attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflicts {
    /// Indicates the conflict transaction hash.
    pub hash: UInt256,
}

impl Conflicts {
    /// Creates a new conflicts attribute.
    pub fn new(hash: UInt256) -> Self {
        Self { hash }
    }

    /// Verify the conflicts attribute.
    pub fn verify(
        &self,
        _settings: &ProtocolSettings,
        snapshot: &DataCache,
        _tx: &super::transaction::Transaction,
    ) -> bool {
        let ledger = LedgerContract::new();
        match ledger.contains_transaction(snapshot, &self.hash) {
            Ok(exists) => !exists,
            Err(err) => {
                warn!(target: "neo", hash = %self.hash, error = %err, "failed to verify conflicts attribute against ledger");
                false
            }
        }
    }

    /// Calculate network fee for this attribute.
    pub fn calculate_network_fee(
        &self,
        base_fee: i64,
        tx: &super::transaction::Transaction,
    ) -> i64 {
        tx.signers().len() as i64 * base_fee
    }

    /// Serialize without type byte.
    pub fn serialize_without_type(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        Serializable::serialize(&self.hash, writer)
    }
}

impl Serializable for Conflicts {
    fn size(&self) -> usize {
        32 // UInt256
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        Serializable::serialize(&self.hash, writer)
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let hash = <UInt256 as Serializable>::deserialize(reader)?;
        Ok(Self { hash })
    }
}
