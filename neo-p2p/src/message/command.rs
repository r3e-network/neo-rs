use core::convert::TryFrom;

use neo_base::encoding::DecodeError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageCommand {
    Version = 0x00,
    Verack = 0x01,
    GetAddr = 0x10,
    Addr = 0x11,
    Ping = 0x18,
    Pong = 0x19,
    GetHeaders = 0x20,
    Headers = 0x21,
    GetBlocks = 0x24,
    Mempool = 0x25,
    Inv = 0x27,
    GetData = 0x28,
    GetBlockByIndex = 0x29,
    NotFound = 0x2A,
    Transaction = 0x2B,
    Block = 0x2C,
    Extensible = 0x2E,
    Reject = 0x2F,
    FilterLoad = 0x30,
    FilterAdd = 0x31,
    FilterClear = 0x32,
    MerkleBlock = 0x38,
    Alert = 0x40,
}

impl MessageCommand {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Version => "version",
            Self::Verack => "verack",
            Self::GetAddr => "getaddr",
            Self::Addr => "addr",
            Self::Ping => "ping",
            Self::Pong => "pong",
            Self::GetHeaders => "getheaders",
            Self::Headers => "headers",
            Self::GetBlocks => "getblocks",
            Self::Mempool => "mempool",
            Self::Inv => "inv",
            Self::GetData => "getdata",
            Self::GetBlockByIndex => "getblockbyindex",
            Self::NotFound => "notfound",
            Self::Transaction => "tx",
            Self::Block => "block",
            Self::Extensible => "extensible",
            Self::Reject => "reject",
            Self::FilterLoad => "filterload",
            Self::FilterAdd => "filteradd",
            Self::FilterClear => "filterclear",
            Self::MerkleBlock => "merkleblock",
            Self::Alert => "alert",
        }
    }

    pub fn allows_compression(self) -> bool {
        matches!(
            self,
            MessageCommand::Block
                | MessageCommand::Extensible
                | MessageCommand::Transaction
                | MessageCommand::Headers
                | MessageCommand::Addr
                | MessageCommand::MerkleBlock
                | MessageCommand::FilterLoad
                | MessageCommand::FilterAdd
        )
    }

    pub fn from_name(name: &str) -> Result<Self, DecodeError> {
        use MessageCommand::*;
        Ok(match name {
            "version" => Version,
            "verack" => Verack,
            "getaddr" => GetAddr,
            "addr" => Addr,
            "ping" => Ping,
            "pong" => Pong,
            "getheaders" => GetHeaders,
            "headers" => Headers,
            "getblocks" => GetBlocks,
            "mempool" => Mempool,
            "inv" => Inv,
            "getdata" => GetData,
            "getblockbyindex" => GetBlockByIndex,
            "notfound" => NotFound,
            "tx" => Transaction,
            "block" => Block,
            "extensible" => Extensible,
            "reject" => Reject,
            "filterload" => FilterLoad,
            "filteradd" => FilterAdd,
            "filterclear" => FilterClear,
            "merkleblock" => MerkleBlock,
            "alert" => Alert,
            _ => return Err(DecodeError::InvalidValue("message command name")),
        })
    }
}

impl TryFrom<u8> for MessageCommand {
    type Error = DecodeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use MessageCommand::*;
        Ok(match value {
            0x00 => Version,
            0x01 => Verack,
            0x10 => GetAddr,
            0x11 => Addr,
            0x18 => Ping,
            0x19 => Pong,
            0x20 => GetHeaders,
            0x21 => Headers,
            0x24 => GetBlocks,
            0x25 => Mempool,
            0x27 => Inv,
            0x28 => GetData,
            0x29 => GetBlockByIndex,
            0x2A => NotFound,
            0x2B => Transaction,
            0x2C => Block,
            0x2E => Extensible,
            0x2F => Reject,
            0x30 => FilterLoad,
            0x31 => FilterAdd,
            0x32 => FilterClear,
            0x38 => MerkleBlock,
            0x40 => Alert,
            _ => return Err(DecodeError::InvalidValue("message command")),
        })
    }
}
