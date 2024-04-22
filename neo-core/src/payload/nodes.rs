// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use alloc::{vec::Vec, string::String};

use neo_base::encoding::bin::*;
use crate::{tx::{Tx, Witness}, types::{Bytes, FixedBytes}};


const MAX_CAPABILITIES: usize = 32;
const MAX_USER_AGENT_SIZE: usize = 1024;
const MAX_IP_ADDR_SIZE: usize = 16;
// const MAX_FILTER_SIZE: usize = 520;


#[derive(Debug, Copy, Clone, BinEncode, BinDecode)]
#[bin(repr = u8)]
pub enum Capability {
    #[bin(tag = 0x01)]
    TcpServer { port: u16 },

    // WsServer { port: u16 }, // deprecated
    #[bin(tag = 0x10)]
    FullNode { start_height: u32 },
}


pub trait NodeCapability {
    fn node_capability(&self) -> Capability;
}


#[derive(Debug, Clone, BinEncode, InnerBinDecode)]
pub struct Version {
    pub network: u32,
    pub version: u32,

    /// i.e unix timestamp in second, UTC
    pub unix_seconds: u32,
    pub nonce: u32,
    pub user_agent: String,
    pub capabilities: Vec<Capability>,
}


impl BinDecoder for Version {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let version = Self::decode_bin_inner(r)?;
        let agent_size = version.user_agent.len();
        if agent_size > MAX_USER_AGENT_SIZE {
            return Err(BinDecodeError::InvalidLength("Version", "user_agent", agent_size));
        }

        let caps = version.capabilities.len();
        if caps > MAX_CAPABILITIES {
            return Err(BinDecodeError::InvalidLength("Version", "capabilities", caps));
        }

        Ok(version)
    }
}


#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct Addr {
    /// i.e unix timestamp in second, UTC
    pub unix_seconds: u32,
    pub ip: FixedBytes<MAX_IP_ADDR_SIZE>,
    pub capabilities: Vec<Capability>,
}


#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct Ping {
    pub last_block_index: u32,

    /// i.e unix timestamp in second, UTC
    pub unix_seconds: u32,
    pub nonce: u32,
}


#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct P2PNotaryRequest {
    pub main_tx: Tx,
    pub fallback_tx: Tx,
    pub witness: Witness,
}


impl EncodeHashFields for P2PNotaryRequest {
    fn encode_hash_fields(&self, w: &mut impl BinWriter) {
        self.main_tx.encode_bin(w);
        self.fallback_tx.encode_bin(w);
    }
}


#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct FilterAdd {
    pub data: Bytes,
}


#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct FilterLoad {
    pub filter: Bytes,
    pub k: u8,
    pub tweak: u32,
}