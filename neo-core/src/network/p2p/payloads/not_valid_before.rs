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

use crate::neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::LedgerContract;
use serde::{Deserialize, Serialize};

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
    pub fn verify(
        &self,
        _settings: &ProtocolSettings,
        snapshot: &DataCache,
        _tx: &super::transaction::Transaction,
    ) -> bool {
        let current_height = LedgerContract::new().current_index(snapshot).unwrap_or(0);
        current_height >= self.height
    }

    /// Serialize without type byte.
    pub fn serialize_without_type(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(self.height)
    }
}

impl Serializable for NotValidBefore {
    fn size(&self) -> usize {
        4 // u32
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(self.height)
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let height = reader.read_u32()?;
        Ok(Self { height })
    }
}
