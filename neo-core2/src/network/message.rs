use std::error::Error;
use std::fmt;

use crate::core::block;
use crate::core::transaction;
use crate::io::{self, BinReader, BinWriter, BufBinWriter};
use crate::network::payload::{self, Payload};

const COMPRESSION_MIN_SIZE: usize = 1024;

#[derive(Debug)]
pub struct Message {
    // Flags that represents whether a message is compressed.
    // 0 for None, 1 for Compressed.
    flags: MessageFlag,
    // Command is a byte command code.
    command: CommandType,
    // Payload sent with the message.
    payload: Box<dyn Payload>,
    // Compressed message payload.
    compressed_payload: Vec<u8>,
    // StateRootInHeader specifies if the state root is included in the block header.
    // This is needed for correct decoding.
    state_root_in_header: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum MessageFlag {
    None = 0,
    Compressed = 1 << 0,
}

#[derive(Debug, Clone, Copy)]
pub enum CommandType {
    CMDVersion = 0x00,
    CMDVerack = 0x01,
    CMDGetAddr = 0x10,
    CMDAddr = 0x11,
    CMDPing = 0x18,
    CMDPong = 0x19,
    CMDGetHeaders = 0x20,
    CMDHeaders = 0x21,
    CMDGetBlocks = 0x24,
    CMDMempool = 0x25,
    CMDInv = 0x27,
    CMDGetData = 0x28,
    CMDGetBlockByIndex = 0x29,
    CMDNotFound = 0x2a,
    CMDTX = payload::TX_TYPE as u8,
    CMDBlock = payload::BLOCK_TYPE as u8,
    CMDExtensible = payload::EXTENSIBLE_TYPE as u8,
    CMDP2PNotaryRequest = payload::P2P_NOTARY_REQUEST_TYPE as u8,
    CMDGetMPTData = 0x51,
    CMDMPTData = 0x52,
    CMDReject = 0x2f,
    CMDFilterLoad = 0x30,
    CMDFilterAdd = 0x31,
    CMDFilterClear = 0x32,
    CMDMerkleBlock = 0x38,
    CMDAlert = 0x40,
}

impl Message {
    pub fn new(cmd: CommandType, p: Box<dyn Payload>) -> Self {
        Self {
            command: cmd,
            payload: p,
            flags: MessageFlag::None,
            compressed_payload: Vec::new(),
            state_root_in_header: false,
        }
    }

    pub fn decode(&mut self, br: &mut BinReader) -> Result<(), Box<dyn Error>> {
        self.flags = MessageFlag::from(br.read_u8()?);
        self.command = CommandType::from(br.read_u8()?);
        let l = br.read_var_uint()?;
        if l == 0 {
            self.payload = match self.command {
                CommandType::CMDFilterClear | CommandType::CMDGetAddr | CommandType::CMDMempool | CommandType::CMDVerack => {
                    Box::new(payload::NullPayload::new())
                }
                _ => return Err(Box::new(fmt::Error::new(fmt::Error, "unexpected empty payload"))),
            };
            return Ok(());
        }
        if l > payload::MAX_SIZE {
            return Err(Box::new(fmt::Error::new(fmt::Error, "invalid payload size")));
        }
        self.compressed_payload = br.read_bytes(l as usize)?;
        self.decode_payload()
    }

    fn decode_payload(&mut self) -> Result<(), Box<dyn Error>> {
        let mut buf = self.compressed_payload.clone();
        if self.flags == MessageFlag::Compressed {
            buf = decompress(&self.compressed_payload)?;
        }

        let p: Box<dyn Payload> = match self.command {
            CommandType::CMDVersion => Box::new(payload::Version::new()),
            CommandType::CMDInv | CommandType::CMDGetData => Box::new(payload::Inventory::new()),
            CommandType::CMDGetMPTData => Box::new(payload::MPTInventory::new()),
            CommandType::CMDMPTData => Box::new(payload::MPTData::new()),
            CommandType::CMDAddr => Box::new(payload::AddressList::new()),
            CommandType::CMDBlock => Box::new(block::Block::new(self.state_root_in_header)),
            CommandType::CMDExtensible => Box::new(payload::Extensible::new()),
            CommandType::CMDP2PNotaryRequest => Box::new(payload::P2PNotaryRequest::new()),
            CommandType::CMDGetBlocks => Box::new(payload::GetBlocks::new()),
            CommandType::CMDGetHeaders | CommandType::CMDGetBlockByIndex => Box::new(payload::GetBlockByIndex::new()),
            CommandType::CMDHeaders => Box::new(payload::Headers::new(self.state_root_in_header)),
            CommandType::CMDTX => {
                let tx = transaction::Transaction::from_bytes(&buf)?;
                self.payload = Box::new(tx);
                return Ok(());
            }
            CommandType::CMDMerkleBlock => Box::new(payload::MerkleBlock::new()),
            CommandType::CMDPing | CommandType::CMDPong => Box::new(payload::Ping::new()),
            CommandType::CMDNotFound => Box::new(payload::Inventory::new()),
            _ => return Err(Box::new(fmt::Error::new(fmt::Error, "can't decode command"))),
        };
        let mut r = BinReader::new(&buf);
        p.decode_binary(&mut r)?;
        self.payload = p;
        Ok(())
    }

    pub fn encode(&self, bw: &mut BinWriter) -> Result<(), Box<dyn Error>> {
        self.try_compress_payload()?;
        let grow_size = 2 + 1 + if !self.compressed_payload.is_empty() { 8 + self.compressed_payload.len() } else { 0 };
        bw.grow(grow_size);
        bw.write_u8(self.flags as u8)?;
        bw.write_u8(self.command as u8)?;
        if !self.compressed_payload.is_empty() {
            bw.write_var_bytes(&self.compressed_payload)?;
        } else {
            bw.write_u8(0)?;
        }
        Ok(())
    }

    pub fn bytes(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut w = BufBinWriter::new();
        self.encode(&mut w)?;
        Ok(w.bytes())
    }

    fn try_compress_payload(&self) -> Result<(), Box<dyn Error>> {
        if self.payload.is_none() {
            return Ok(());
        }
        let mut buf = BufBinWriter::new();
        self.payload.encode_binary(&mut buf)?;
        let mut compressed_payload = buf.bytes();
        if self.flags == MessageFlag::None {
            match self.payload.as_ref() {
                payload::Headers | payload::MerkleBlock | payload::NullPayload | payload::Inventory | payload::MPTInventory => {}
                _ => {
                    let size = compressed_payload.len();
                    if size > COMPRESSION_MIN_SIZE {
                        compressed_payload = compress(&compressed_payload)?;
                        self.flags = MessageFlag::Compressed;
                    }
                }
            }
        }
        self.compressed_payload = compressed_payload;
        Ok(())
    }
}

fn decompress(data: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    // Implement decompression logic here
    Ok(data.to_vec())
}

fn compress(data: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    // Implement compression logic here
    Ok(data.to_vec())
}
