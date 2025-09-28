// Copyright (C) 2015-2025 The Neo Project.
//
// high_priority_attribute.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::neo_io::{MemoryReader, Serializable};
use crate::persistence::DataCache;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

/// Indicates that the transaction is of high priority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighPriorityAttribute;

impl HighPriorityAttribute {
    /// Creates a new high priority attribute.
    pub fn new() -> Self {
        Self
    }

    /// Verify the high priority attribute.
    pub fn verify(&self, snapshot: &DataCache, tx: &super::transaction::Transaction) -> bool {
        // TODO: Check if any signer is the committee address when committee methods are available
        // let committee_address = snapshot.get_committee_address();
        // tx.signers.iter().any(|s| s.account == committee_address)
        true
    }

    /// Serialize without type byte.
    pub fn serialize_without_type(&self, _writer: &mut dyn Write) -> io::Result<()> {
        Ok(()) // No data to serialize
    }
}

impl Default for HighPriorityAttribute {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializable for HighPriorityAttribute {
    fn size(&self) -> usize {
        0 // No additional data
    }

    fn serialize(&self, _writer: &mut dyn Write) -> io::Result<()> {
        Ok(()) // No data to serialize
    }

    fn deserialize(_reader: &mut MemoryReader) -> Result<Self, String> {
        Ok(Self) // No data to deserialize
    }
}
