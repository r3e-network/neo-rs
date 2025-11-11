mod capability;

pub use capability::{Capability, CapabilityType};

use std::collections::BTreeSet;

use neo_base::encoding::{
    read_varint, write_varint, DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite,
};

pub const MAX_CAPABILITIES: u64 = 32;
pub const MAX_USER_AGENT_LEN: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionPayload {
    pub network: u32,
    pub version: u32,
    pub timestamp: u32,
    pub nonce: u32,
    pub user_agent: String,
    pub capabilities: Vec<Capability>,
}

impl VersionPayload {
    pub fn new(
        network: u32,
        version: u32,
        timestamp: u32,
        nonce: u32,
        user_agent: String,
        capabilities: Vec<Capability>,
    ) -> Self {
        debug_assert!(
            user_agent.len() <= MAX_USER_AGENT_LEN,
            "user agent exceeds {} bytes",
            MAX_USER_AGENT_LEN
        );
        debug_assert!(
            capabilities.len() as u64 <= MAX_CAPABILITIES,
            "too many capabilities (max {})",
            MAX_CAPABILITIES
        );
        debug_assert!(
            !has_duplicate_known_capabilities(&capabilities),
            "duplicate capability type"
        );
        Self {
            network,
            version,
            timestamp,
            nonce,
            user_agent,
            capabilities,
        }
    }

    pub fn allows_compression(&self) -> bool {
        !self
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::DisableCompression))
    }
}

impl NeoEncode for VersionPayload {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.network.neo_encode(writer);
        self.version.neo_encode(writer);
        self.timestamp.neo_encode(writer);
        self.nonce.neo_encode(writer);
        self.user_agent.neo_encode(writer);
        write_varint(writer, self.capabilities.len() as u64);
        for capability in &self.capabilities {
            capability.neo_encode(writer);
        }
    }
}

impl NeoDecode for VersionPayload {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let network = u32::neo_decode(reader)?;
        let version = u32::neo_decode(reader)?;
        let timestamp = u32::neo_decode(reader)?;
        let nonce = u32::neo_decode(reader)?;
        let user_agent = String::neo_decode(reader)?;
        if user_agent.len() > MAX_USER_AGENT_LEN {
            return Err(DecodeError::LengthOutOfRange {
                len: user_agent.len() as u64,
                max: MAX_USER_AGENT_LEN as u64,
            });
        }
        let capability_count = read_varint(reader)?;
        if capability_count > MAX_CAPABILITIES {
            return Err(DecodeError::LengthOutOfRange {
                len: capability_count,
                max: MAX_CAPABILITIES,
            });
        }
        let mut capabilities = Vec::with_capacity(capability_count as usize);
        for _ in 0..capability_count {
            capabilities.push(Capability::neo_decode(reader)?);
        }
        if has_duplicate_known_capabilities(&capabilities) {
            return Err(DecodeError::InvalidValue("duplicate capability type"));
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingPayload {
    pub last_block_index: u32,
    pub timestamp: u32,
    pub nonce: u32,
}

impl NeoEncode for PingPayload {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.last_block_index.neo_encode(writer);
        self.timestamp.neo_encode(writer);
        self.nonce.neo_encode(writer);
    }
}

impl NeoDecode for PingPayload {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(PingPayload {
            last_block_index: u32::neo_decode(reader)?,
            timestamp: u32::neo_decode(reader)?,
            nonce: u32::neo_decode(reader)?,
        })
    }
}

fn has_duplicate_known_capabilities(capabilities: &[Capability]) -> bool {
    let mut seen = BTreeSet::new();
    for capability in capabilities {
        let ty = capability.capability_type();
        if matches!(ty, CapabilityType::Unknown(_) | CapabilityType::Extension0) {
            continue;
        }
        if !seen.insert(ty) {
            return true;
        }
    }
    false
}
