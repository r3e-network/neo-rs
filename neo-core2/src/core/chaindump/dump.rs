use std::fmt;
use std::io::{self, Read, Write};

use crate::config::BlockchainConfig;
use crate::core::block::Block;
use crate::util::Uint256;

// DumperRestorer is a trait to get/add blocks from/to.
pub trait DumperRestorer {
    fn add_block(&self, block: &Block) -> Result<(), Box<dyn std::error::Error>>;
    fn get_block(&self, hash: Uint256) -> Result<Block, Box<dyn std::error::Error>>;
    fn get_config(&self) -> BlockchainConfig;
    fn get_header_hash(&self, index: u32) -> Uint256;
}

// Dump writes count blocks from start to the provided writer.
// Note: header needs to be written separately by a client.
pub fn dump<D: DumperRestorer, W: Write>(bc: &D, w: &mut W, start: u32, count: u32) -> Result<(), Box<dyn std::error::Error>> {
    let mut buf = Vec::new();

    for i in start..start + count {
        let bh = bc.get_header_hash(i);
        let b = bc.get_block(bh)?;
        b.encode_binary(&mut buf)?;
        let bytes = buf.clone();
        w.write_all(&(bytes.len() as u32).to_le_bytes())?;
        w.write_all(&bytes)?;
        buf.clear();
    }
    Ok(())
}

// Restore restores blocks from the provided reader.
// f is called after addition of every block.
pub fn restore<D: DumperRestorer, R: Read, F>(bc: &D, r: &mut R, skip: u32, count: u32, f: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: Fn(&Block) -> Result<(), Box<dyn std::error::Error>>,
{
    let mut buf = Vec::new();

    let mut read_block = |r: &mut R| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut size_buf = [0u8; 4];
        r.read_exact(&mut size_buf)?;
        let size = u32::from_le_bytes(size_buf) as usize;
        buf.resize(size, 0);
        r.read_exact(&mut buf)?;
        Ok(buf.clone())
    };

    for _ in 0..skip {
        read_block(r)?;
    }

    let state_root_in_header = bc.get_config().state_root_in_header;

    for i in skip..skip + count {
        let buf = read_block(r)?;
        let mut b = Block::new(state_root_in_header);
        let mut r = io::Cursor::new(buf);
        b.decode_binary(&mut r)?;
        if b.index != 0 || i != 0 || skip != 0 {
            bc.add_block(&b)?;
        }
        f(&b)?;
    }
    Ok(())
}
