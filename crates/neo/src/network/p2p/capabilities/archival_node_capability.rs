// Copyright (C) 2015-2025 The Neo Project.
//
// archival_node_capability.rs file belongs to the neo project and is free
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

/// Indicates that a node is an archival node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchivalNodeCapability;

impl ArchivalNodeCapability {
    /// Initializes a new instance of the ArchivalNodeCapability class.
    pub fn new() -> Self {
        Self
    }

    /// Get the capability type.
    pub fn capability_type(&self) -> NodeCapabilityType {
        NodeCapabilityType::ArchivalNode
    }
}

impl Default for ArchivalNodeCapability {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializable for ArchivalNodeCapability {
    fn size(&self) -> usize {
        1 // Type only
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(&[NodeCapabilityType::ArchivalNode as u8])
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let _type = reader.read_u8().map_err(|e| e.to_string())?;
        Ok(Self)
    }
}

/// Helper function to create an ArchivalNodeCapability.
pub fn archival_node() -> ArchivalNodeCapability {
    ArchivalNodeCapability::new()
}
