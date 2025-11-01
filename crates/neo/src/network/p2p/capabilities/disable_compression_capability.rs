// Copyright (C) 2015-2025 The Neo Project.
//
// disable_compression_capability.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::node_capability_type::NodeCapabilityType;
use crate::neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

/// Indicates that a node does not support compression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisableCompressionCapability;

impl DisableCompressionCapability {
    /// Initializes a new instance of the DisableCompressionCapability class.
    pub fn new() -> Self {
        Self
    }

    /// Get the capability type.
    pub fn capability_type(&self) -> NodeCapabilityType {
        NodeCapabilityType::DisableCompression
    }
}

impl Default for DisableCompressionCapability {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializable for DisableCompressionCapability {
    fn size(&self) -> usize {
        1 // Type only
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u8(NodeCapabilityType::DisableCompression.to_byte())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let _ = reader.read_u8()?;
        Ok(Self)
    }
}

/// Helper function to create a DisableCompressionCapability.
pub fn disable_compression() -> DisableCompressionCapability {
    DisableCompressionCapability::new()
}
