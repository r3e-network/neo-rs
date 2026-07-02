//! Per-peer `GetBlockByIndex` request scheduling.
//!
//! This module extracts the C# `TaskManager` request-window policy into a pure
//! type that peer sessions can use without embedding cursor arithmetic in
//! socket code.

/// One `GetBlockByIndex` request planned for a peer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockRequest {
    /// First block index requested.
    pub start: u32,
    /// Number of blocks requested.
    pub count: u32,
}

impl BlockRequest {
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

/// Per-peer block request scheduler.
///
/// It plans request ranges only; the owning session still serializes and sends
/// the wire message.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BlockRequestScheduler {
    requested_to: u32,
    last_local_height: u32,
    stall_ticks: u32,
}

impl BlockRequestScheduler {
    /// Maximum block hashes allowed by one `GetBlockByIndex` request.
    pub const MAX_BLOCKS_PER_REQUEST: u32 = 500;
    /// Maximum in-flight request distance ahead of the durable local height.
    pub const MAX_BLOCKS_AHEAD: u32 = 1_000;
    /// Consecutive no-progress sync ticks before the in-flight cursor rewinds.
    pub const STALL_LIMIT: u32 = 15;

    /// Highest block index already requested from this peer.
    #[must_use]
    pub const fn requested_to(&self) -> u32 {
        self.requested_to
    }

    /// Consecutive no-progress ticks while the peer is ahead.
    #[must_use]
    pub const fn stall_ticks(&self) -> u32 {
        self.stall_ticks
    }

    /// Record one sync tick for stall detection.
    pub fn record_tick(&mut self, local_height: u32, peer_height: u32) {
        if peer_height <= local_height {
            self.requested_to = local_height;
            self.last_local_height = local_height;
            self.stall_ticks = 0;
            return;
        }

        if local_height == self.last_local_height {
            self.stall_ticks = self.stall_ticks.saturating_add(1);
        } else {
            self.stall_ticks = 0;
            self.last_local_height = local_height;
        }
    }

    /// Plan the next request for a peer that advertises `peer_height`.
    ///
    /// Returns `None` when the peer is caught up to us or the per-peer
    /// in-flight window is already full.
    #[must_use]
    pub fn next_request(&mut self, local_height: u32, peer_height: u32) -> Option<BlockRequest> {
        if peer_height <= local_height {
            self.requested_to = local_height;
            self.last_local_height = local_height;
            self.stall_ticks = 0;
            return None;
        }

        if self.stall_ticks >= Self::STALL_LIMIT
            || self.requested_to > local_height.saturating_add(Self::MAX_BLOCKS_AHEAD)
        {
            self.requested_to = local_height;
            self.stall_ticks = 0;
        }

        let start = local_height
            .saturating_add(1)
            .max(self.requested_to.saturating_add(1));
        let request_window_end = local_height.saturating_add(Self::MAX_BLOCKS_AHEAD);
        if start > peer_height || start > request_window_end {
            return None;
        }

        let upper = peer_height.min(request_window_end);
        let count = upper
            .saturating_sub(start)
            .saturating_add(1)
            .min(Self::MAX_BLOCKS_PER_REQUEST);
        let request = BlockRequest::new(start, count);
        self.requested_to = request.end();
        Some(request)
    }
}
