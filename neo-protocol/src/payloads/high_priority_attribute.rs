// Copyright (C) 2015-2025 The Neo Project.
// Licensed under the MIT License. See LICENSE file for details.

use neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

/// Indicates that the transaction is of high priority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighPriorityAttribute;

impl HighPriorityAttribute {
    /// Creates a high priority attribute.
    pub fn new() -> Self {
        Self
    }
    /// Serializes the empty body without its type byte.
    pub fn serialize_without_type(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        <Self as Serializable>::serialize(self, writer)
    }
}
impl Default for HighPriorityAttribute {
    fn default() -> Self {
        Self::new()
    }
}
impl Serializable for HighPriorityAttribute {
    fn size(&self) -> usize {
        0
    }
    fn serialize(&self, _writer: &mut BinaryWriter) -> IoResult<()> {
        Ok(())
    }
    fn deserialize(_reader: &mut MemoryReader) -> IoResult<Self> {
        Ok(Self)
    }
}
