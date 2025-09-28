// Copyright (C) 2015-2025 The Neo Project.
//
// version_payload.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::neo_io::{MemoryReader, Serializable};
use crate::network::p2p::capabilities::{DisableCompressionCapability, NodeCapability};
use crate::network::p2p::local_node::LocalNode;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

/// Indicates the maximum number of capabilities contained in a VersionPayload.
pub const MAX_CAPABILITIES: usize = 32;

/// Sent when a connection is established.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

    /// True if allow compression
    pub allow_compression: bool,

    /// The capabilities of the node.
    pub capabilities: Vec<NodeCapability>,
}

impl VersionPayload {
    /// Creates a new instance of the VersionPayload class.
    pub fn create(
        network: u32,
        nonce: u32,
        user_agent: String,
        capabilities: Vec<NodeCapability>,
    ) -> Self {
        let allow_compression = !capabilities
            .iter()
            .any(|c| matches!(c, NodeCapability::DisableCompression(_)));

        Self {
            network,
            version: LocalNode::PROTOCOL_VERSION,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as u32,
            nonce,
            user_agent,
            allow_compression,
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
        self.user_agent.len() + 1 + // UserAgent with var length prefix
        1 + self.capabilities.iter().map(|c| c.size()).sum::<usize>() // Capabilities
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(&self.network.to_le_bytes())?;
        writer.write_all(&self.version.to_le_bytes())?;
        writer.write_all(&self.timestamp.to_le_bytes())?;
        writer.write_all(&self.nonce.to_le_bytes())?;

        // Write user agent as var string
        let user_agent_bytes = self.user_agent.as_bytes();
        if user_agent_bytes.len() > 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "User agent too long",
            ));
        }
        writer.write_all(&[user_agent_bytes.len() as u8])?;
        writer.write_all(user_agent_bytes)?;

        // Write capabilities
        if self.capabilities.len() > MAX_CAPABILITIES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Too many capabilities",
            ));
        }
        writer.write_all(&[self.capabilities.len() as u8])?;
        for capability in &self.capabilities {
            capability.serialize(writer)?;
        }

        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let network = reader.read_u32().map_err(|e| e.to_string())?;
        let version = reader.read_u32().map_err(|e| e.to_string())?;
        let timestamp = reader.read_u32().map_err(|e| e.to_string())?;
        let nonce = reader.read_u32().map_err(|e| e.to_string())?;
        let user_agent = reader.read_var_string(1024).map_err(|e| e.to_string())?;

        // Read capabilities
        let capability_count = reader.read_var_int().map_err(|e| e.to_string())?;
        if capability_count > MAX_CAPABILITIES as u64 {
            return Err("Too many capabilities".to_string());
        }

        let mut capabilities = Vec::with_capacity(capability_count as usize);
        for _ in 0..capability_count {
            capabilities.push(NodeCapability::deserialize_from(reader)?);
        }

        // Check for duplicate capability types (excluding UnknownCapability)
        let mut seen_types = std::collections::HashSet::new();
        for capability in &capabilities {
            if !matches!(capability, NodeCapability::Unknown(_)) {
                let cap_type = capability.get_type();
                if !seen_types.insert(cap_type) {
                    return Err("Duplicate capability type".to_string());
                }
            }
        }

        let allow_compression = !capabilities
            .iter()
            .any(|c| matches!(c, NodeCapability::DisableCompression(_)));

        Ok(Self {
            network,
            version,
            timestamp,
            nonce,
            user_agent,
            allow_compression,
            capabilities,
        })
    }
}
