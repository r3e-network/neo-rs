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

use crate::neo_io::{MemoryReader, Serializable};
use crate::{persistence::DataCache, UInt256};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

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
    pub fn verify(&self, snapshot: &DataCache, _tx: &super::transaction::Transaction) -> bool {
        // TODO: Check if conflicting transaction is on chain when DataCache methods are available
        // Only check if conflicting transaction is on chain. It's OK if the
        // conflicting transaction was in the Conflicts attribute of some other
        // on-chain transaction.
        // !snapshot.contains_transaction(&self.hash)
        true
    }

    /// Calculate network fee for this attribute.
    pub fn calculate_network_fee(
        &self,
        _snapshot: &DataCache,
        tx: &super::transaction::Transaction,
    ) -> i64 {
        // Fee is multiplied by number of signers
        tx.signers().len() as i64 * 1000000 // Base fee in datoshi
    }

    /// Serialize without type byte.
    pub fn serialize_without_type(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.hash.serialize(writer)
    }
}

impl Serializable for Conflicts {
    fn size(&self) -> usize {
        32 // UInt256
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.hash.serialize(writer)
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let hash = UInt256::deserialize(reader)?;
        Ok(Self { hash })
    }
}
