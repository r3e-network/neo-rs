use neo::extensions::*;
use neo::io::*;
use neo::network::p2p::capabilities::NodeCapability;
use std::io::{Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};
use byteorder::LittleEndian;
use NeoRust::prelude::{StringExt, VarSizeTrait};
use crate::io::iserializable::ISerializable;
use crate::network::Capabilities::NodeCapability;
use crate::network::LocalNode;

/// Sent when a connection is established.
pub struct VersionPayload {
    /// The magic number of the network.
    pub network: u32,

    /// The protocol version of the node.
    pub version: u32,

    /// The time when connected to the node.
    pub timestamp: u32,

    /// A random number used to identify the node.
    pub nonce: u32,

    /// A string used to identify the client software of the node.
    pub user_agent: String,

    /// The capabilities of the node.
    pub capabilities: Vec<dyn NodeCapability>,
}

/// Indicates the maximum number of capabilities contained in a VersionPayload.
pub const MAX_CAPABILITIES: usize = 32;

impl VersionPayload {
    pub fn size(&self) -> usize {
        std::mem::size_of::<u32>() * 4 + // Network + Version + Timestamp + Nonce
        self.user_agent.var_size() +
        self.capabilities.var_size()
    }

    /// Creates a new instance of the VersionPayload struct.
    ///
    /// # Arguments
    ///
    /// * `network` - The magic number of the network.
    /// * `nonce` - The random number used to identify the node.
    /// * `user_agent` - The string used to identify the client software of the node.
    /// * `capabilities` - The capabilities of the node.
    ///
    /// # Returns
    ///
    /// The created payload.
    pub fn create(network: u32, nonce: u32, user_agent: String, capabilities: Vec<dyn NodeCapability>) -> Self {
        Self {
            network,
            version: LocalNode::PROTOCOL_VERSION,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_secs() as u32,
            nonce,
            user_agent,
            capabilities,
        }
    }
}

impl ISerializable for VersionPayload {
    fn deserialize<R: Read>(&mut self, reader: &mut R) -> std::io::Result<()> {
        self.network = reader.read_u32::<LittleEndian>()?;
        self.version = reader.read_u32::<LittleEndian>()?;
        self.timestamp = reader.read_u32::<LittleEndian>()?;
        self.nonce = reader.read_u32::<LittleEndian>()?;
        self.user_agent = reader.read_var_string(1024)?;

        let cap_count = reader.read_var_int(MAX_CAPABILITIES)?;
        self.capabilities = Vec::with_capacity(cap_count);
        for _ in 0..cap_count {
            self.capabilities.push(NodeCapability::deserialize_from(reader)?);
        }

        if self.capabilities.iter().map(|c| c.get_type()).collect::<std::collections::HashSet<_>>().len() != self.capabilities.len() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Duplicate capability types"));
        }

        Ok(())
    }

    fn serialize<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_u32::<LittleEndian>(self.network)?;
        writer.write_u32::<LittleEndian>(self.version)?;
        writer.write_u32::<LittleEndian>(self.timestamp)?;
        writer.write_u32::<LittleEndian>(self.nonce)?;
        writer.write_var_string(&self.user_agent)?;
        writer.write_var_int(self.capabilities.len() as u64)?;
        for capability in &self.capabilities {
            capability.serialize(writer)?;
        }
        Ok(())
    }

    fn size(&self) -> usize {
        todo!()
    }
}
