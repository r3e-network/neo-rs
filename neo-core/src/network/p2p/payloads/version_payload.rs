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

use crate::macros::ValidateLength;
use crate::neo_io::serializable::helper::get_var_size;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::network::p2p::capabilities::NodeCapability;

/// Protocol version constant (moved from LocalNode for decoupling)
pub const PROTOCOL_VERSION: u32 = 0;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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
            .any(|c| matches!(c, NodeCapability::DisableCompression));

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
        get_var_size(self.user_agent.len() as u64)
            + self.user_agent.len()
            + get_var_size(self.capabilities.len() as u64)
            + self.capabilities.iter().map(|c| c.size()).sum::<usize>()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(self.network)?;
        writer.write_u32(self.version)?;
        writer.write_u32(self.timestamp)?;
        writer.write_u32(self.nonce)?;

        // Use ValidateLength trait to reduce boilerplate
        self.user_agent.validate_max_length(1024, "User agent")?;
        writer.write_var_string(&self.user_agent)?;

        self.capabilities
            .validate_max_length(MAX_CAPABILITIES, "Capabilities")?;
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

        let allow_compression = !capabilities
            .iter()
            .any(|c| matches!(c, NodeCapability::DisableCompression));

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
