//! Downloaded block batch values.
//!
//! A batch records the supplying peer, first height, and canonical block bodies.
//! Runtime sync code converts it into `neo_runtime::SyncBlockBatch` before
//! import.

use neo_payloads::{Block, Header};

use crate::PeerId;

/// One contiguous batch yielded by a block downloader.
#[derive(Clone, Debug)]
pub struct BlockDownloadBatch {
    /// Peer that supplied this batch, when known.
    pub peer_id: Option<PeerId>,
    /// Height of the first block in `blocks`.
    pub start_height: u32,
    /// Downloaded blocks in canonical order.
    pub blocks: Vec<Block>,
}

impl BlockDownloadBatch {
    /// Construct a downloaded batch.
    #[must_use]
    pub fn new(peer_id: Option<PeerId>, start_height: u32, blocks: Vec<Block>) -> Self {
        Self {
            peer_id,
            start_height,
            blocks,
        }
    }

    /// Height immediately after the last block in this batch.
    #[must_use]
    pub fn next_height(&self) -> u32 {
        self.start_height
            .saturating_add(u32::try_from(self.blocks.len()).unwrap_or(u32::MAX))
    }

    /// Returns `true` when this batch carries no blocks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

impl From<BlockDownloadBatch> for neo_runtime::SyncBlockBatch {
    fn from(batch: BlockDownloadBatch) -> Self {
        Self::new(batch.start_height, batch.blocks)
    }
}

/// One contiguous batch yielded by a header downloader.
#[derive(Clone, Debug)]
pub struct HeaderDownloadBatch {
    /// Peer that supplied this batch, when known.
    pub peer_id: Option<PeerId>,
    /// Height of the first header in `headers`.
    pub start_height: u32,
    /// Downloaded headers in canonical order.
    pub headers: Vec<Header>,
}

impl HeaderDownloadBatch {
    /// Construct a downloaded batch.
    #[must_use]
    pub fn new(peer_id: Option<PeerId>, start_height: u32, headers: Vec<Header>) -> Self {
        Self {
            peer_id,
            start_height,
            headers,
        }
    }

    /// Height immediately after the last header in this batch.
    #[must_use]
    pub fn next_height(&self) -> u32 {
        self.start_height
            .saturating_add(u32::try_from(self.headers.len()).unwrap_or(u32::MAX))
    }

    /// Returns `true` when this batch carries no headers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.headers.is_empty()
    }
}
