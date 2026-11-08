//! Aggregate per-peer session state for the ported protocol handlers.
//!
//! Mirrors the mutable fields `neo-core`'s `RemoteNode` carried for a single
//! connection (`known_hashes`, `sent_hashes`, `pending_known_hashes`,
//! `bloom_filter`, `sent_commands`, handshake flags, `last_block_index`, and the
//! peer `endpoint`). The handler modules ([`crate::inventory`],
//! [`crate::peer_messages`]) operate on `&mut SessionState` plus the typed
//! [`crate::ports::RemoteSessionPorts`] bundle, so this struct owns only the
//! connection-local dedup/filter/command bookkeeping — never an actor, system,
//! ledger, or storage type.

use crate::bloom_filter::BloomFilterState;
use crate::handshake::{HandshakeGateDecision, handshake_gate_decision};
use crate::message_command::MessageCommand;
use crate::outbound_queue::CommandBitSet;
use crate::pending_known_hashes::PendingKnownHashes;
use neo_io::HashSetCache;
use neo_primitives::UInt256;
use std::net::SocketAddr;

/// Default capacity for the per-session dedup caches.
pub const DEFAULT_SESSION_CACHE_CAPACITY: usize = 1024;

/// Connection-local mutable state for one peer session.
#[derive(Debug)]
pub struct SessionState {
    /// Hashes already known/processed for this peer (bounded LRU set).
    pub known_hashes: HashSetCache<UInt256>,
    /// Hashes already sent to this peer in reply to `GetData` (bounded LRU set).
    pub sent_hashes: HashSetCache<UInt256>,
    /// Hashes recently announced and pending processing (TTL FIFO).
    pub pending_known_hashes: PendingKnownHashes,
    /// Bloom filter state governing filtered-block replies.
    pub bloom_filter: BloomFilterState,
    /// Commands already sent and awaiting a correlated reply (e.g. `GetAddr`).
    pub sent_commands: CommandBitSet,
    /// Highest block index the peer has advertised.
    pub last_block_index: u32,
    /// Whether a `Version` message has been received.
    pub version_received: bool,
    /// Whether the version/verack handshake has completed.
    pub handshake_complete: bool,
    /// Remote socket endpoint as seen by the transport layer.
    pub endpoint: SocketAddr,
}

impl SessionState {
    /// Creates a new session state with the given cache capacity and endpoint.
    pub fn new(capacity: usize, endpoint: SocketAddr) -> Self {
        Self {
            known_hashes: HashSetCache::new(capacity),
            sent_hashes: HashSetCache::new(capacity),
            pending_known_hashes: PendingKnownHashes::new(capacity),
            bloom_filter: BloomFilterState::default(),
            sent_commands: CommandBitSet::default(),
            last_block_index: 0,
            version_received: false,
            handshake_complete: false,
            endpoint,
        }
    }

    /// Creates a new session state with the default cache capacity.
    pub fn with_endpoint(endpoint: SocketAddr) -> Self {
        Self::new(DEFAULT_SESSION_CACHE_CAPACITY, endpoint)
    }

    /// Whether an announced hash should be skipped (already pending or known).
    ///
    /// Mirrors `RemoteNode::should_skip_inventory`.
    pub fn should_skip_inventory(&self, hash: &UInt256) -> bool {
        self.pending_known_hashes.contains(hash) || self.known_hashes.contains(hash)
    }

    /// Consumes a previously-sent command flag, returning whether it was set.
    ///
    /// Used to correlate a reply (e.g. `Addr`) with the request that asked for
    /// it (`GetAddr`); an unsolicited reply is ignored.
    pub fn consume_sent_command(&mut self, command: MessageCommand) -> bool {
        self.sent_commands.take(command)
    }

    /// Records that a command was sent so its reply can be correlated.
    pub fn record_sent_command(&mut self, command: MessageCommand) {
        self.sent_commands.insert(command);
    }

    /// Evaluates the handshake gate for an inbound command.
    pub fn handshake_gate(&self, command: MessageCommand) -> HandshakeGateDecision {
        handshake_gate_decision(self.version_received, self.handshake_complete, command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::Instant;

    fn endpoint() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 20333)
    }

    #[test]
    fn should_skip_inventory_tracks_pending_and_known_hashes() {
        let mut state = SessionState::with_endpoint(endpoint());
        let hash = UInt256::from([1u8; 32]);

        assert!(!state.should_skip_inventory(&hash));

        state.pending_known_hashes.try_add(hash, Instant::now());
        assert!(state.should_skip_inventory(&hash));

        let other = UInt256::from([2u8; 32]);
        state.known_hashes.try_add(other);
        assert!(state.should_skip_inventory(&other));
    }

    #[test]
    fn sent_command_flags_are_consumed_once() {
        let mut state = SessionState::with_endpoint(endpoint());

        state.record_sent_command(MessageCommand::GetAddr);
        assert!(state.consume_sent_command(MessageCommand::GetAddr));
        assert!(!state.consume_sent_command(MessageCommand::GetAddr));
    }

    #[test]
    fn handshake_gate_reflects_session_progress() {
        let mut state = SessionState::with_endpoint(endpoint());

        assert_eq!(
            state.handshake_gate(MessageCommand::Version),
            HandshakeGateDecision::AcceptVersion
        );

        state.version_received = true;
        assert_eq!(
            state.handshake_gate(MessageCommand::Verack),
            HandshakeGateDecision::AcceptVerack
        );

        state.handshake_complete = true;
        assert_eq!(
            state.handshake_gate(MessageCommand::Ping),
            HandshakeGateDecision::AcceptProtocol
        );
    }
}
