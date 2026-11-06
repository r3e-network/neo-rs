// Copyright (C) 2015-2025 The Neo Project.
// Licensed under the MIT License. See LICENSE file for details.
use neo_io::{BinaryWriter, IoResult, Serializable, impl_serializable};
use serde::{Deserialize, Serialize};

/// A not-valid-before transaction attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotValidBefore {
    /// The first valid block height.
    pub height: u32,
}
impl NotValidBefore {
    /// Creates an attribute for the specified height.
    pub fn new(height: u32) -> Self {
        Self { height }
    }
    /// Serializes the body without its type byte.
    pub fn serialize_without_type(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        <Self as Serializable>::serialize(self, writer)
    }
}
impl_serializable! { struct NotValidBefore { height: u32, } }
