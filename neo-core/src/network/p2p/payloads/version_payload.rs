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

use crate::cryptography::ECPoint;
use crate::macros::ValidateLength;
use crate::neo_io::serializable::helper::get_var_size;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::network::p2p::capabilities::NodeCapability;
use crate::protocol_settings::ProtocolSettings;
use crate::wallets::KeyPair;
use crate::UInt256;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Protocol version constant
pub const PROTOCOL_VERSION: u32 = 0;

/// Indicates the maximum number of capabilities contained in a VersionPayload.
pub const MAX_CAPABILITIES: usize = 32;

/// Prefix for NodeId calculation
const NODE_ID_PREFIX: &[u8] = b"NEO_DHT_NODEID";

/// Sent when a connection is established.
/// Matches C# VersionPayload from Neo v3.9.2
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionPayload {
    /// The magic number of the network.
    pub network: u32,

    /// The protocol version of the node.
    pub version: u32,

    /// The time when connected to the node (UTC).
    pub timestamp: u32,

    /// The public key associated with this node.
    pub node_key: ECPoint,

    /// The unique identifier for the node.
    pub node_id: UInt256,

    /// A string used to identify the client software of the node.
    pub user_agent: String,

    /// True if allow compression
    pub allow_compression: bool,

    /// The capabilities of the node.
    pub capabilities: Vec<NodeCapability>,

    /// The digital signature of the payload.
    pub signature: Vec<u8>,
}

impl VersionPayload {
    /// Creates a new instance of the VersionPayload class with signature.
    /// Matches C# VersionPayload.Create method
    pub fn create(
        settings: &ProtocolSettings,
        node_key: &KeyPair,
        user_agent: String,
        capabilities: Vec<NodeCapability>,
    ) -> Self {
        let public_key = match node_key.get_public_key_point() {
            Ok(pk) => pk,
            Err(e) => {
                tracing::warn!(target: "neo::p2p", "Failed to get public key: {}, using empty key", e);
                // Use a placeholder that will fail verification but allow payload creation
                ECPoint::from_bytes(&[2u8; 33]).expect("valid compressed point")
            }
        };
        let node_id = Self::compute_node_id(settings, &public_key);
        let allow_compression = !capabilities
            .iter()
            .any(|c| matches!(c, NodeCapability::DisableCompression));

        let payload = Self {
            network: settings.network,
            version: PROTOCOL_VERSION,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
                .min(u32::MAX as u64) as u32,
            node_key: public_key.clone(),
            node_id,
            user_agent,
            allow_compression,
            capabilities,
            signature: Vec::new(),
        };

        // Generate signature
        let mut payload_bytes = Vec::new();
        payload
            .serialize_with_signature(&mut payload_bytes)
            .unwrap();
        let signature = node_key.sign(&payload_bytes).unwrap_or_default();

        let mut signed = payload;
        signed.signature = signature;
        signed
    }

    /// Computes the NodeId from a public key.
    /// Matches C# ECPoint.GetNodeId extension method
    pub fn compute_node_id(settings: &ProtocolSettings, public_key: &ECPoint) -> UInt256 {
        let key_bytes = public_key.as_bytes();
        let mut data = Vec::with_capacity(NODE_ID_PREFIX.len() + 4 + key_bytes.len());
        data.extend_from_slice(NODE_ID_PREFIX);
        data.extend_from_slice(&settings.network.to_le_bytes());
        data.extend_from_slice(key_bytes);

        neo_crypto::crypto_utils::NeoHash::sha256(&neo_crypto::crypto_utils::NeoHash::sha256(&data))
            .into()
    }

    /// Verifies the VersionPayload signature.
    /// Matches C# VersionPayload.Verify method
    pub fn verify(&self, settings: &ProtocolSettings) -> bool {
        // Verify NodeId matches NodeKey
        if self.node_id != Self::compute_node_id(settings, &self.node_key) {
            return false;
        }

        // Verify signature
        if self.signature.is_empty() {
            return false;
        }

        let mut payload_bytes = Vec::new();
        if self.serialize_with_signature(&mut payload_bytes).is_err() {
            return false;
        }

        self.node_key
            .verify_signature(&payload_bytes, &self.signature)
            .unwrap_or(false)
    }

    /// Serializes including signature
    pub fn serialize_with_signature(&self, writer: &mut Vec<u8>) -> IoResult<()> {
        use crate::neo_io::BinaryWriter;
        let mut binary_writer = BinaryWriter::new();

        // Serialize without signature first
        binary_writer.write_u32(self.network)?;
        binary_writer.write_u32(self.version)?;
        binary_writer.write_u32(self.timestamp)?;

        // Serialize ECPoint as raw bytes
        let key_bytes = self.node_key.as_bytes();
        binary_writer.write_bytes(key_bytes)?;

        // Serialize UInt256 using Serializable trait
        binary_writer.write_serializable(&self.node_id)?;

        binary_writer.write_var_string(&self.user_agent)?;

        self.capabilities
            .validate_max_length(MAX_CAPABILITIES, "Capabilities")?;
        binary_writer.write_var_uint(self.capabilities.len() as u64)?;
        for capability in &self.capabilities {
            binary_writer.write_serializable(capability)?;
        }

        // Write signature
        binary_writer.write_var_bytes(&self.signature)?;

        writer.extend_from_slice(&binary_writer.into_bytes());

        Ok(())
    }

    /// Gets the size including signature
    pub fn size_with_signature(&self) -> usize {
        4 + // Network
        4 + // Version
        4 + // Timestamp
        self.node_key.as_bytes().len() + // NodeKey
        32 + // UInt256 (NodeId)
        get_var_size(self.user_agent.len() as u64) + self.user_agent.len() +
        get_var_size(self.capabilities.len() as u64) +
        self.capabilities.iter().map(|c| c.size()).sum::<usize>() +
        get_var_size(self.signature.len() as u64) + self.signature.len()
    }
}

impl Default for VersionPayload {
    fn default() -> Self {
        // Create an empty/invalid ECPoint for Default
        let empty_key = ECPoint::from_bytes(&[0u8; 33]).unwrap_or_else(|_| {
            ECPoint::decode_compressed_with_curve(
                crate::cryptography::ECCurve::secp256r1(),
                &[0u8; 33],
            )
            .unwrap_or_else(|_| {
                ECPoint::from_bytes(&[2u8; 33])
                    .unwrap_or_else(|e| panic!("Failed to create default ECPoint: {}", e))
            })
        });

        Self {
            network: 0,
            version: 0,
            timestamp: 0,
            node_key: empty_key,
            node_id: UInt256::default(),
            user_agent: String::new(),
            allow_compression: true,
            capabilities: Vec::new(),
            signature: Vec::new(),
        }
    }
}

impl Serializable for VersionPayload {
    fn size(&self) -> usize {
        4 + // Network
        4 + // Version
        4 + // Timestamp
        self.node_key.as_bytes().len() + // NodeKey
        32 + // UInt256 (NodeId)
        get_var_size(self.user_agent.len() as u64)
            + self.user_agent.len()
            + get_var_size(self.capabilities.len() as u64)
            + self.capabilities.iter().map(|c| c.size()).sum::<usize>()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(self.network)?;
        writer.write_u32(self.version)?;
        writer.write_u32(self.timestamp)?;

        // Serialize ECPoint as raw bytes
        let key_bytes = self.node_key.as_bytes();
        writer.write_bytes(key_bytes)?;

        // Serialize UInt256 using Serializable trait
        writer.write_serializable(&self.node_id)?;

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

        // Deserialize ECPoint - read 33 bytes for compressed secp256r1/k1
        let key_len = reader.peek()? as usize;
        let key_data = if key_len == 0x02 || key_len == 0x03 {
            33
        } else if key_len == 0x04 || key_len == 0x06 || key_len == 0x07 {
            65
        } else {
            return Err(IoError::invalid_data("Invalid ECPoint prefix"));
        };
        let key_bytes = reader.read_bytes(key_data)?;
        let node_key = ECPoint::from_bytes(&key_bytes)
            .map_err(|_| IoError::invalid_data("Invalid ECPoint data"))?;

        // Deserialize UInt256
        let node_id_data = reader.read_bytes(32)?;
        let node_id = UInt256::from_bytes(&node_id_data)
            .map_err(|_| IoError::invalid_data("Invalid UInt256 data"))?;

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

        // Read signature if present (for newer protocol versions)
        let signature = if reader.remaining() > 0 {
            reader.read_var_bytes(65536)?.to_vec()
        } else {
            Vec::new()
        };

        let allow_compression = !capabilities
            .iter()
            .any(|c| matches!(c, NodeCapability::DisableCompression));

        Ok(Self {
            network,
            version,
            timestamp,
            node_key,
            node_id,
            user_agent,
            allow_compression,
            capabilities,
            signature,
        })
    }
}
