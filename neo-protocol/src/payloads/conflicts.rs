// Copyright (C) 2015-2025 The Neo Project.
// Licensed under the MIT License. See LICENSE file for details.
use neo_io::{BinaryWriter, IoResult, Serializable, impl_serializable};
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// A transaction conflict declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflicts {
    /// The conflicting transaction hash.
    pub hash: UInt256,
}
impl Conflicts {
    /// Creates a conflict declaration.
    pub fn new(hash: UInt256) -> Self {
        Self { hash }
    }
    /// Serializes the body without its type byte.
    pub fn serialize_without_type(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        <Self as Serializable>::serialize(self, writer)
    }
}
impl_serializable! { struct Conflicts { hash: UInt256, } }
