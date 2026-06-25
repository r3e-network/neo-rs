//! chain.acc block-file importer.
//!
//! Reads serialized blocks from a `chain.acc` dump file (the same format C# Neo
//! uses for cold-start bootstrap) and imports them in batches via the existing
//! `BlockchainCommand::Import { blocks, verify: false }`. This bypasses the P2P
//! network entirely — the single biggest cold-start speedup.
//!
//! File format (C# `MainService.GetBlocks`):
//! ```text
//! u32 count              // number of blocks
//! repeat count:
//!   i32 size             // serialized block length
//!   [u8; size]           // Neo-serialized Block
//! ```
//! Optional variant with a start-height prefix:
//! ```text
//! u32 start              // first block height
//! u32 count
//! ```

use std::path::Path;

use neo_blockchain::command::BlockchainCommand;
use neo_blockchain::handle::BlockchainHandle;
use neo_io::{MemoryReader, Serializable};
use neo_payloads::block::Block;
use tracing::{info, warn};

/// The batch size for Import commands. C# Neo uses 10; we use 500 since our
/// per-block persist is only ~0.5ms and the batch amortizes the channel round-trip.
const IMPORT_BATCH_SIZE: usize = 500;

/// Import blocks from a `chain.acc` file. Returns the number of blocks imported.
pub async fn import_chain_acc(
    handle: &BlockchainHandle,
    path: &Path,
    verify: bool,
) -> anyhow::Result<u64> {
    let bytes = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("reading chain.acc {}: {e}", path.display()))?;
    let mut reader = MemoryReader::new(&bytes);

    // Read optional start height + count.
    let first_u32 = read_u32_le(&mut reader)?;
    let second_u32 = read_u32_le(&mut reader)?;

    // Heuristic: if first_u32 > 0 and second_u32 is a plausible count (< 10M),
    // assume the file has a start-height prefix. Otherwise treat first_u32 as count.
    let (count, _start) = if first_u32 > 0 && second_u32 < 10_000_000 {
        (second_u32 as usize, first_u32)
    } else {
        reader = MemoryReader::new(&bytes);
        let c = read_u32_le(&mut reader)? as usize;
        (c, 0u32)
    };

    info!(target: "neo::import", file = %path.display(), count, verify, "importing blocks from chain.acc");

    let mut batch: Vec<Block> = Vec::with_capacity(IMPORT_BATCH_SIZE);
    let mut imported = 0u64;

    for i in 0..count {
        let size = read_i32_le(&mut reader)?;
        if size <= 0 || size > 0x0200_0000 {
            warn!(target: "neo::import", index = i, size, "invalid block size, stopping import");
            break;
        }
        let block_bytes = reader.read_bytes(size as usize)?;
        let block = Block::deserialize(&mut MemoryReader::new(&block_bytes))
            .map_err(|e| anyhow::anyhow!("deserializing block {i}: {e}"))?;
        batch.push(block);

        if batch.len() >= IMPORT_BATCH_SIZE || i == count - 1 {
            let batch_blocks = std::mem::take(&mut batch);
            let batch_len = batch_blocks.len();
            handle
                .tell(BlockchainCommand::Import(
                    neo_blockchain::import::Import {
                        blocks: batch_blocks,
                        verify,
                    },
                ))
                .await
                .map_err(|e| anyhow::anyhow!("import command send failed: {e}"))?;
            imported += batch_len as u64;
            if imported % 10_000 == 0 {
                info!(target: "neo::import", imported, total = count, "import progress");
            }
        }
    }

    info!(target: "neo::import", imported, "chain.acc import complete");
    Ok(imported)
}

fn read_u32_le(reader: &mut MemoryReader) -> anyhow::Result<u32> {
    let bytes = reader.read_bytes(4)
        .map_err(|e| anyhow::anyhow!("reading u32: {e}"))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_i32_le(reader: &mut MemoryReader) -> anyhow::Result<i32> {
    let bytes = reader.read_bytes(4)
        .map_err(|e| anyhow::anyhow!("reading i32: {e}"))?;
    Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
