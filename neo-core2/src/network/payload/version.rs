use std::time::{SystemTime, UNIX_EPOCH};
use crate::config::netmode::Magic;
use crate::io::{BinReader, BinWriter};
use crate::network::capability::{Capabilities, Capability};

// MaxUserAgentLength is the limit for the user agent field.
const MAX_USER_AGENT_LENGTH: usize = 1024;

// Version payload.
pub struct Version {
    // NetMode of the node
    pub magic: Magic,
    // currently the version of the protocol is 0
    pub version: u32,
    // timestamp
    pub timestamp: u32,
    // it's used to distinguish several nodes using the same public IP (or different ones)
    pub nonce: u32,
    // client id
    pub user_agent: Vec<u8>,
    // List of available network services
    pub capabilities: Capabilities,
}

impl Version {
    // NewVersion returns a pointer to a Version payload.
    pub fn new(magic: Magic, id: u32, ua: &str, c: Vec<Capability>) -> Version {
        Version {
            magic,
            version: 0,
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as u32,
            nonce: id,
            user_agent: ua.as_bytes().to_vec(),
            capabilities: Capabilities(c),
        }
    }

    // DecodeBinary implements the Serializable interface.
    pub fn decode_binary(&mut self, br: &mut BinReader) {
        self.magic = Magic(br.read_u32_le());
        self.version = br.read_u32_le();
        self.timestamp = br.read_u32_le();
        self.nonce = br.read_u32_le();
        self.user_agent = br.read_var_bytes(MAX_USER_AGENT_LENGTH);
        self.capabilities.decode_binary(br);
    }

    // EncodeBinary implements the Serializable interface.
    pub fn encode_binary(&self, bw: &mut BinWriter) {
        bw.write_u32_le(self.magic.0);
        bw.write_u32_le(self.version);
        bw.write_u32_le(self.timestamp);
        bw.write_u32_le(self.nonce);
        bw.write_var_bytes(&self.user_agent);
        self.capabilities.encode_binary(bw);
    }
}
