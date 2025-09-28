// Copyright (C) 2015-2025 The Neo Project.
//
// full_node_capability.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::node_capability_type::NodeCapabilityType;
use crate::neo_io::{MemoryReader, Serializable};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

/// Indicates that a node has complete current state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FullNodeCapability {
    /// Indicates the current block height of the node.
    pub start_height: u32,
}

impl FullNodeCapability {
    /// Initializes a new instance of the FullNodeCapability class.
    pub fn new(start_height: u32) -> Self {
        Self { start_height }
    }

    /// Get the capability type.
    pub fn capability_type(&self) -> NodeCapabilityType {
        NodeCapabilityType::FullNode
    }
}

impl Serializable for FullNodeCapability {
    fn size(&self) -> usize {
        1 + 4 // Type + StartHeight
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(&[NodeCapabilityType::FullNode as u8])?;
        writer.write_all(&self.start_height.to_le_bytes())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let _type = reader.read_u8().map_err(|e| e.to_string())?;
        let start_height = reader.read_u32().map_err(|e| e.to_string())?;
        Ok(Self { start_height })
    }
}

/// Helper function to create a FullNodeCapability.
pub fn full_node(start_height: u32) -> FullNodeCapability {
    FullNodeCapability::new(start_height)
}
