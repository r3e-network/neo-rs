// Copyright (C) 2015-2025 The Neo Project.
//
// not_valid_before.rs file belongs to the neo project and is free
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

/// Represents a not-valid-before transaction attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotValidBefore {
    /// Indicates that the transaction is not valid before this height.
    pub height: u32,
}

impl NotValidBefore {
    /// Creates a new not-valid-before attribute.
    pub fn new(height: u32) -> Self {
        Self { height }
    }

    /// Verify the not-valid-before attribute.
    pub fn verify(&self, snapshot: &DataCache, _tx: &super::transaction::Transaction) -> bool {
        // TODO: Get current block height when DataCache methods are available
        // let block_height = snapshot.get_current_block_height();
        // block_height >= self.height
        true
    }

    /// Serialize without type byte.
    pub fn serialize_without_type(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(&self.height.to_le_bytes())
    }
}

impl Serializable for NotValidBefore {
    fn size(&self) -> usize {
        4 // u32
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(&self.height.to_le_bytes())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let height = reader.read_u32().map_err(|e| e.to_string())?;
        Ok(Self { height })
    }
}
