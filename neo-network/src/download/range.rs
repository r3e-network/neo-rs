//! Cross-peer block range assignment.
//!
//! `CrossPeerBlockRangeScheduler` is the transport-independent policy object
//! used by a future P2P stream downloader. It assigns contiguous block ranges
//! across eligible peers, caps in-flight work, preserves peer bias, and retries
//! failed ranges on another peer before surfacing an error.

use std::collections::{BTreeMap, VecDeque};

use super::{BlockDownloadConfig, BlockRequest, BlockRequestScheduler};
use crate::{NetworkError, NetworkResult, PeerId};

/// Snapshot of a peer that can serve block ranges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockDownloadPeer {
    /// Peer identifier.
    pub peer_id: PeerId,
    /// Highest block height the peer advertises.
    pub height: u32,
}

impl BlockDownloadPeer {
    /// Construct a peer snapshot for range scheduling.
    #[must_use]
    pub const fn new(peer_id: PeerId, height: u32) -> Self {
        Self { peer_id, height }
    }
}

/// One assigned request range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockRangeAssignment {
    /// Peer selected for the request.
    pub peer_id: PeerId,
    /// Block range to request.
    pub request: BlockRequest,
    /// Zero-based attempt number for this exact range.
    pub attempt: usize,
}

impl BlockRangeAssignment {
    /// Construct an assignment.
    #[must_use]
    pub const fn new(peer_id: PeerId, request: BlockRequest, attempt: usize) -> Self {
        Self {
            peer_id,
            request,
            attempt,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RetryRange {
    request: BlockRequest,
    attempt: usize,
    failed_peers: Vec<PeerId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InFlightRange {
    assignment: BlockRangeAssignment,
    failed_peers: Vec<PeerId>,
}

/// Retry-aware scheduler for assigning block ranges across peers.
///
/// The scheduler is deliberately pure. It does not fetch from sockets or
/// inspect returned blocks; callers report success or failure for each
/// assignment and keep the transport-specific work outside this policy layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CrossPeerBlockRangeScheduler {
    config: BlockDownloadConfig,
    target_height: u32,
    next_start: u32,
    in_flight: BTreeMap<u32, InFlightRange>,
    retries: VecDeque<RetryRange>,
}

impl CrossPeerBlockRangeScheduler {
    /// Build a scheduler for `(local_height, target_height]`.
    #[must_use]
    pub fn new(local_height: u32, target_height: u32, config: BlockDownloadConfig) -> Self {
        Self {
            config,
            target_height,
            next_start: local_height.saturating_add(1),
            in_flight: BTreeMap::new(),
            retries: VecDeque::new(),
        }
    }

    /// Scheduling config.
    #[must_use]
    pub const fn config(&self) -> &BlockDownloadConfig {
        &self.config
    }

    /// Next fresh block height that has not been assigned.
    #[must_use]
    pub const fn next_start(&self) -> u32 {
        self.next_start
    }

    /// Number of ranges currently assigned and awaiting completion.
    #[must_use]
    pub fn in_flight_len(&self) -> usize {
        self.in_flight.len()
    }

    /// Returns `true` once no fresh, retry, or in-flight range remains.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.next_start > self.target_height && self.in_flight.is_empty() && self.retries.is_empty()
    }

    /// Assign the next range to an eligible peer.
    ///
    /// Retries are assigned before fresh ranges. Returns `None` when the
    /// in-flight limit is full, no peer can serve the next range, or the target
    /// height is already scheduled.
    pub fn next_assignment(&mut self, peers: &[BlockDownloadPeer]) -> Option<BlockRangeAssignment> {
        if self.in_flight.len() >= self.config.max_concurrency {
            return None;
        }

        if let Some(assignment) = self.next_retry_assignment(peers) {
            return Some(assignment);
        }

        if self.next_start > self.target_height {
            return None;
        }

        let peer = Self::select_peer(
            self.config.peer_bias,
            peers,
            self.next_start,
            self.next_start,
            &[],
        )?;
        let count = self.fresh_request_count(peer.height);
        if count == 0 {
            return None;
        }

        let request = BlockRequest::new(self.next_start, count);
        self.next_start = request.end().saturating_add(1);
        let assignment = BlockRangeAssignment::new(peer.peer_id, request, 0);
        self.in_flight.insert(
            request.start,
            InFlightRange {
                assignment,
                failed_peers: Vec::new(),
            },
        );
        Some(assignment)
    }

    /// Mark an assignment as successfully delivered.
    pub fn record_success(&mut self, assignment: BlockRangeAssignment) -> NetworkResult<()> {
        let Some(in_flight) = self.in_flight.get(&assignment.request.start) else {
            return Err(Self::unknown_assignment_error(assignment));
        };
        if in_flight.assignment != assignment {
            return Err(Self::unknown_assignment_error(assignment));
        }
        self.in_flight.remove(&assignment.request.start);
        Ok(())
    }

    /// Mark an assignment as failed and enqueue it for retry if allowed.
    pub fn record_failure(&mut self, assignment: BlockRangeAssignment) -> NetworkResult<()> {
        let Some(in_flight) = self.in_flight.remove(&assignment.request.start) else {
            return Err(Self::unknown_assignment_error(assignment));
        };
        if in_flight.assignment != assignment {
            self.in_flight
                .insert(in_flight.assignment.request.start, in_flight);
            return Err(Self::unknown_assignment_error(assignment));
        }
        if assignment.attempt >= self.config.retry_limit {
            return Err(NetworkError::Protocol(format!(
                "block range {}..={} failed after {} attempts",
                assignment.request.start,
                assignment.request.end(),
                assignment.attempt.saturating_add(1)
            )));
        }

        let mut failed_peers = in_flight.failed_peers;
        failed_peers.push(assignment.peer_id);
        self.retries.push_front(RetryRange {
            request: assignment.request,
            attempt: assignment.attempt.saturating_add(1),
            failed_peers,
        });
        Ok(())
    }

    fn next_retry_assignment(
        &mut self,
        peers: &[BlockDownloadPeer],
    ) -> Option<BlockRangeAssignment> {
        let retry_count = self.retries.len();
        for _ in 0..retry_count {
            let retry = self.retries.pop_front()?;
            if let Some(peer) = Self::select_peer(
                self.config.peer_bias,
                peers,
                retry.request.start,
                retry.request.end(),
                &retry.failed_peers,
            ) {
                let assignment =
                    BlockRangeAssignment::new(peer.peer_id, retry.request, retry.attempt);
                self.in_flight.insert(
                    retry.request.start,
                    InFlightRange {
                        assignment,
                        failed_peers: retry.failed_peers,
                    },
                );
                return Some(assignment);
            }
            self.retries.push_back(retry);
        }
        None
    }

    fn fresh_request_count(&self, peer_height: u32) -> u32 {
        if peer_height < self.next_start {
            return 0;
        }
        let max_by_config = u32::try_from(self.config.max_batch_size).unwrap_or(u32::MAX);
        let max_count = max_by_config.min(BlockRequestScheduler::MAX_BLOCKS_PER_REQUEST);
        let upper = self
            .target_height
            .min(peer_height)
            .min(self.next_start.saturating_add(max_count).saturating_sub(1));
        upper.saturating_sub(self.next_start).saturating_add(1)
    }

    fn select_peer(
        peer_bias: Option<PeerId>,
        peers: &[BlockDownloadPeer],
        start: u32,
        end: u32,
        excluded: &[PeerId],
    ) -> Option<BlockDownloadPeer> {
        let mut best = None;
        for peer in peers {
            if peer.height < start || peer.height < end || excluded.contains(&peer.peer_id) {
                continue;
            }
            if peer_bias == Some(peer.peer_id) {
                return Some(*peer);
            }
            if best.is_none_or(|current: BlockDownloadPeer| {
                peer.height > current.height
                    || (peer.height == current.height && peer.peer_id < current.peer_id)
            }) {
                best = Some(*peer);
            }
        }
        best
    }

    fn unknown_assignment_error(assignment: BlockRangeAssignment) -> NetworkError {
        NetworkError::Protocol(format!(
            "unknown block range assignment {}..={} for {} attempt {}",
            assignment.request.start,
            assignment.request.end(),
            assignment.peer_id,
            assignment.attempt
        ))
    }
}
