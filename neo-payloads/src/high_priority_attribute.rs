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

use neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

/// Indicates that the transaction is of high priority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighPriorityAttribute;

impl HighPriorityAttribute {
    /// Creates a new high priority attribute.
    pub fn new() -> Self {
        Self
    }

    /// Verify the high priority attribute.

    /// Serialize without type byte.
    pub fn serialize_without_type(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        <Self as Serializable>::serialize(self, writer)
    }
}

// Use macro to reduce boilerplate
neo_io::impl_default_via_new!(HighPriorityAttribute);

impl Serializable for HighPriorityAttribute {
    fn size(&self) -> usize {
        0 // No additional data
    }

    fn serialize(&self, _writer: &mut BinaryWriter) -> IoResult<()> {
        Ok(()) // No data to serialize
    }

    fn deserialize(_reader: &mut MemoryReader) -> IoResult<Self> {
        Ok(Self) // No data to deserialize
    }
}
