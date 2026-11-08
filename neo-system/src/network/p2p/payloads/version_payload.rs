// Copyright (C) 2015-2025 The Neo Project.
//
// version_payload.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms, with or without
// modifications are permitted.

use super::node_capability::{
    NodeCapability, deserialize_node_capabilities, node_capabilities_size,
    serialize_node_capabilities,
};
use neo_io::serializable::helper::get_var_size_str;
use neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

/// Protocol version constant
pub const PROTOCOL_VERSION: u32 = 0;

/// Indicates the maximum number of capabilities contained in a VersionPayload.
pub const MAX_CAPABILITIES: usize = 32;

/// Sent when a connection is established.
/// Matches C# VersionPayload exactly
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VersionPayload {
    /// The magic number of the network.
    pub network: u32,

    /// The protocol version of the node.
    pub version: u32,

    /// The time when connected to the node (UTC).
    pub timestamp: u32,

    /// A random number used to identify the node.
    pub nonce: u32,

    /// A string used to identify the client software of the node.
    pub user_agent: String,

    /// The capabilities of the node.
    pub capabilities: Vec<NodeCapability>,
}

impl VersionPayload {
    /// Creates a new instance of the VersionPayload class.
    /// Matches C# VersionPayload.Create method.
    pub fn create(
        network: u32,
        nonce: u32,
        user_agent: String,
        capabilities: Vec<NodeCapability>,
    ) -> Self {
        Self {
            network,
            version: PROTOCOL_VERSION,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
                .min(u32::MAX as u64) as u32,
            nonce,
            user_agent,
            capabilities,
        }
    }
}

impl Serializable for VersionPayload {
    fn size(&self) -> usize {
        4 + // Network
        4 + // Version
        4 + // Timestamp
        4 + // Nonce
        get_var_size_str(&self.user_agent) + // UserAgent
        node_capabilities_size(&self.capabilities)
        // Capabilities
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(self.network)?;
        writer.write_u32(self.version)?;
        writer.write_u32(self.timestamp)?;
        writer.write_u32(self.nonce)?;
        writer.write_var_string(&self.user_agent)?;
        serialize_node_capabilities(&self.capabilities, writer)
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let network = reader.read_u32()?;
        let version = reader.read_u32()?;
        let timestamp = reader.read_u32()?;
        let nonce = reader.read_u32()?;
        let user_agent = reader.read_var_string(1024)?;

        let capabilities = deserialize_node_capabilities(reader, MAX_CAPABILITIES)?;

        Ok(Self {
            network,
            version,
            timestamp,
            nonce,
            user_agent,
            capabilities,
        })
    }
}
