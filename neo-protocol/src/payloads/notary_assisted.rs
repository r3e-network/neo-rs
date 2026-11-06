// Copyright (C) 2015-2025 The Neo Project.
// Licensed under the MIT License. See LICENSE file for details.
use neo_io::{BinaryWriter, IoResult, Serializable, impl_serializable};
use serde::{Deserialize, Serialize};

/// A notary-assisted transaction attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotaryAssisted {
    /// Number of keys participating in signing.
    pub nkeys: u8,
}
impl NotaryAssisted {
    /// Creates an attribute for the specified key count.
    pub fn new(nkeys: u8) -> Self {
        Self { nkeys }
    }
    /// Serializes the body without its type byte.
    pub fn serialize_without_type(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        <Self as Serializable>::serialize(self, writer)
    }
}
impl_serializable! { struct NotaryAssisted { nkeys: u8, } }
