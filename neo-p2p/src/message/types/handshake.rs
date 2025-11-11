use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use super::Endpoint;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionPayload {
    pub network: u32,
    pub protocol: u32,
    pub services: u64,
    pub timestamp: i64,
    pub receiver: Endpoint,
    pub sender: Endpoint,
    pub nonce: u64,
    pub user_agent: String,
    pub start_height: u32,
    pub relay: bool,
}

impl VersionPayload {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        network: u32,
        protocol: u32,
        services: u64,
        timestamp: i64,
        receiver: Endpoint,
        sender: Endpoint,
        nonce: u64,
        user_agent: String,
        start_height: u32,
        relay: bool,
    ) -> Self {
        Self {
            network,
            protocol,
            services,
            timestamp,
            receiver,
            sender,
            nonce,
            user_agent,
            start_height,
            relay,
        }
    }
}

impl NeoEncode for VersionPayload {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.network.neo_encode(writer);
        self.protocol.neo_encode(writer);
        self.services.neo_encode(writer);
        self.timestamp.neo_encode(writer);
        self.receiver.neo_encode(writer);
        self.sender.neo_encode(writer);
        self.nonce.neo_encode(writer);
        self.user_agent.neo_encode(writer);
        self.start_height.neo_encode(writer);
        self.relay.neo_encode(writer);
    }
}

impl NeoDecode for VersionPayload {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            network: u32::neo_decode(reader)?,
            protocol: u32::neo_decode(reader)?,
            services: u64::neo_decode(reader)?,
            timestamp: i64::neo_decode(reader)?,
            receiver: Endpoint::neo_decode(reader)?,
            sender: Endpoint::neo_decode(reader)?,
            nonce: u64::neo_decode(reader)?,
            user_agent: String::neo_decode(reader)?,
            start_height: u32::neo_decode(reader)?,
            relay: bool::neo_decode(reader)?,
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
