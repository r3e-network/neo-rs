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

use crate::neo_io::serializable::helper::get_var_size;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::network::p2p::capabilities::NodeCapability;
use crate::protocol_settings::ProtocolSettings;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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
    /// Matches C# VersionPayload.Create method
    pub fn create(
        settings: &ProtocolSettings,
        nonce: u32,
        user_agent: String,
        capabilities: Vec<NodeCapability>,
    ) -> Self {
        Self {
            network: settings.network,
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
        get_var_size(self.user_agent.len() as u64) + self.user_agent.len() + // UserAgent
        get_var_size(self.capabilities.len() as u64) + self.capabilities.iter().map(|c| c.size()).sum::<usize>()
        // Capabilities
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(self.network)?;
        writer.write_u32(self.version)?;
        writer.write_u32(self.timestamp)?;
        writer.write_u32(self.nonce)?;
        writer.write_var_string(&self.user_agent)?;
        writer.write_var_uint(self.capabilities.len() as u64)?;
        for capability in &self.capabilities {
            writer.write_serializable(capability)?;
        }
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let network = reader.read_u32()?;
        let version = reader.read_u32()?;
        let timestamp = reader.read_u32()?;
        let nonce = reader.read_u32()?;
        let user_agent = reader.read_var_string(1024)?;

        // Read capabilities
        let capability_count = reader.read_var_int(MAX_CAPABILITIES as u64)? as usize;
        let mut capabilities = Vec::with_capacity(capability_count);
        for _ in 0..capability_count {
            capabilities.push(<NodeCapability as Serializable>::deserialize(reader)?);
        }

        // Check for duplicate capability types (excluding UnknownCapability)
        let mut seen_types = HashSet::new();
        for capability in &capabilities {
            if !matches!(capability, NodeCapability::Unknown { .. }) {
                let cap_type = capability.capability_type();
                if !seen_types.insert(cap_type) {
                    return Err(IoError::invalid_data("Duplicate capability type"));
                }
            }
        }

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
