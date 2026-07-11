//! `GetBlockByIndex` request values and protocol limits.

/// One `GetBlockByIndex` request planned for a peer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockRequest {
    /// First block index requested.
    pub start: u32,
    /// Number of blocks requested.
    pub count: u32,
}

impl BlockRequest {
    /// Maximum blocks accepted by one Neo `GetBlockByIndex` request.
    pub const MAX_COUNT: u32 = neo_payloads::inv_payload::MAX_HASHES_COUNT as u32;

    /// Construct a block request.
    #[must_use]
    pub const fn new(start: u32, count: u32) -> Self {
        Self { start, count }
    }

    /// Last block index covered by this request.
    #[must_use]
    pub const fn end(self) -> u32 {
        self.start.saturating_add(self.count.saturating_sub(1))
    }
}
