use super::capabilities::NodeCapability;
use crate::{NetworkError, NetworkResult as Result};
use neo_io::{helper, BinaryWriter, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

/// Maximum number of capabilities (matches C# MaxCapabilities)
pub const MAX_CAPABILITIES: usize = 32;

/// Version payload for handshake (matches C# VersionPayload exactly)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionPayload {
    /// The magic number of the network (matches C# Network property)
    pub network: u32,

    /// The protocol version of the node (matches C# Version property)
    pub version: u32,

    /// The time when connected to the node UTC (matches C# Timestamp property)
    pub timestamp: u32,

    /// A random number used to identify the node (matches C# Nonce property)
    pub nonce: u32,

    /// String used to identify the client software (matches C# UserAgent property)
    pub user_agent: String,

    /// True if compression is allowed (matches C# AllowCompression property)
    pub allow_compression: bool,

    /// The capabilities of the node (matches C# Capabilities property)
    pub capabilities: Vec<NodeCapability>,
}

impl VersionPayload {
    /// Create new VersionPayload (matches C# constructor)
    pub fn new(network: u32, nonce: u32, user_agent: String) -> Self {
        Self {
            network,
            version: 0, // Current Neo protocol version
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as u32,
            nonce,
            user_agent,
            allow_compression: true, // Enable compression by default
            capabilities: Vec::new(),
        }
    }

    /// Add capability (matches C# capability management)
    pub fn add_capability(&mut self, capability: NodeCapability) -> Result<()> {
        if self.capabilities.len() >= MAX_CAPABILITIES {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "version".to_string(),
                reason: format!("Too many capabilities: {}", self.capabilities.len()),
            });
        }

        self.capabilities.push(capability);
        Ok(())
    }

    /// Add TCP server capability (convenience method)
    pub fn add_tcp_server_capability(&mut self, port: u16) -> Result<()> {
        self.add_capability(NodeCapability::tcp_server(port))
    }

    /// Add full node capability (convenience method)
    pub fn add_full_node_capability(&mut self, start_height: u32) -> Result<()> {
        self.add_capability(NodeCapability::full_node(start_height))
    }
}

impl Serializable for VersionPayload {
    /// Deserialize VersionPayload (matches C# ISerializable.Deserialize exactly)
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let network = reader.read_u32()?;
        let version = reader.read_u32()?;
        let timestamp = reader.read_u32()?;
        let nonce = reader.read_u32()?;

        // Read UserAgent as VarString (matches C# ReadVarString)
        let user_agent = reader.read_var_string(1024)?;

        // AllowCompression is not serialized in the current protocol; default to true.
        let allow_compression = true;

        // Capabilities (matches C# ReadSerializableArray)
        let capabilities = helper::deserialize_array::<NodeCapability>(reader, MAX_CAPABILITIES)?;

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

    /// Serialize VersionPayload (matches C# ISerializable.Serialize exactly)
    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(self.network)?;
        writer.write_u32(self.version)?;
        writer.write_u32(self.timestamp)?;
        writer.write_u32(self.nonce)?;

        // Write UserAgent as VarString (matches C# WriteVarString)
        writer.write_var_string(&self.user_agent)?;

        // Capabilities (matches C# WriteSerializableArray)
        helper::serialize_array(&self.capabilities, writer)?;

        Ok(())
    }

    /// Get size in bytes (matches C# Size property exactly)
    fn size(&self) -> usize {
        4 + // network
        4 + // version  
        4 + // timestamp
        4 + // nonce
        helper::get_var_size(self.user_agent.len() as u64) + self.user_agent.len() +
        helper::get_array_size(&self.capabilities)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_payload_creation() {
        let payload = VersionPayload::new(0x334F454E, 12345, "neo-rust/0.4.0".to_string());

        assert_eq!(payload.network, 0x334F454E);
        assert_eq!(payload.nonce, 12345);
        assert_eq!(payload.user_agent, "neo-rust/0.4.0");
        assert_eq!(payload.version, 0);
        assert!(payload.allow_compression);
    }

    #[test]
    fn test_version_payload_serialization() {
        let mut payload = VersionPayload::new(0x334F454E, 12345, "neo-rust/0.4.0".to_string());
        payload.add_tcp_server_capability(10333).unwrap();
        payload.add_full_node_capability(1000).unwrap();

        // Test serialization roundtrip
        let serialized = payload.to_array();
        let deserialized = VersionPayload::from_array(&serialized).unwrap();

        assert_eq!(payload.network, deserialized.network);
        assert_eq!(payload.nonce, deserialized.nonce);
        assert_eq!(payload.user_agent, deserialized.user_agent);
        assert_eq!(payload.capabilities.len(), deserialized.capabilities.len());
    }
}
