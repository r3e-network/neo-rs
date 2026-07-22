//! Shared connected-peer registry with admission control.
//!
//! The single source of truth for "who is connected", shared by the
//! [`crate::LocalNodeService`] command loop (dial path),
//! its accept loop (inbound path), and every spawned
//! [`crate::remote_node::RemoteNodeService`] (self-removal on exit and
//! the duplicate-connection filter). Replaces the earlier split-brain
//! arrangement where dialed peers lived in a service-private map and
//! accepted peers in a separate accept-loop map.
//!
//! Mirrors the C# state it stands in for:
//!
//! - `Peer.ConnectedPeers` + `Peer.ConnectedAddresses` with the
//!   `MaxConnections` / `MaxConnectionsPerAddress` admission checks of
//!   `Peer.OnTcpConnected` (`Peer.cs:272-301`).
//! - The duplicate-connection filter of
//!   `LocalNode.AllowNewConnection` (`LocalNode.cs:166-168`): a second
//!   connection from the same remote address whose version nonce
//!   matches an existing peer's is rejected.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use crate::ChannelsConfig;

use crate::download::{
    BlockDownloadBatch, BlockDownloadPeer, BlockRangeAssignment, BlockRangeFetcher,
};
use crate::error::{NetworkError, NetworkResult};
use crate::peer_id::PeerId;
use crate::remote_node::RemoteNodeHandle;

/// Per-peer record held by the registry.
struct PeerEntry {
    /// Command handle of the peer's service task.
    handle: RemoteNodeHandle,
    /// Transport-level remote address (the accepted source endpoint
    /// for inbound peers, the dialed endpoint for outbound peers).
    remote_addr: SocketAddr,
    /// Version-payload nonce, recorded once the peer's version
    /// message has been received (C# `RemoteNode.Version.Nonce`).
    version_nonce: Option<u32>,
    /// Advertised listener endpoint (remote IP + the `TcpServer`
    /// capability port from the peer's version), recorded once the
    /// handshake completes. `None` until then or for peers that
    /// advertise no listener (C# `RemoteNode.ListenerTcpPort == 0`).
    /// Used to answer `GetAddr` (C# `OnGetAddrMessageReceived` gossips
    /// `p.Listener`, not the ephemeral source endpoint).
    listener_addr: Option<SocketAddr>,
    /// Last block height advertised by this peer's FullNode capability or
    /// ping/pong payload.
    last_block_index: Option<u32>,
    /// Whether the peer completed version/verack processing and can accept
    /// coordinator-assigned data-plane requests.
    ready: bool,
    /// Whether the connection was accepted locally. Inbound peers report an
    /// unknown listener port until their version payload upgrades it.
    inbound: bool,
}

/// Interior, lock-guarded state.
struct RegistryInner {
    peers: HashMap<PeerId, PeerEntry>,
    /// Live connection count per remote IP address
    /// (C# `Peer.ConnectedAddresses`).
    address_counts: HashMap<IpAddr, usize>,
    /// Candidate peer endpoints learned from `Addr` gossip but not yet
    /// dialed (C# `Peer.UnconnectedPeers`). The dial path
    /// (`LocalNodeService`'s discovery tick) drains from here when the
    /// connected count drops below the desired minimum. Bounded by
    /// [`PeerRegistry::UNCONNECTED_MAX`] (C# `Peer.UnconnectedMax`).
    unconnected: HashSet<SocketAddr>,
}

/// Shared connected-peer registry with C#-faithful admission control.
pub struct PeerRegistry {
    /// Hard cap on simultaneously connected peers
    /// (C# `ChannelsConfig.MaxConnections`, default 40). The C# `-1`
    /// "unlimited" sentinel has no `usize` equivalent; `usize::MAX`
    /// expresses the same thing.
    max_connections: usize,
    /// Per-remote-address connection cap
    /// (C# `ChannelsConfig.MaxConnectionsPerAddress`, default 3).
    max_connections_per_address: usize,
    inner: parking_lot::Mutex<RegistryInner>,
}

/// Authoritative read-side snapshot of one live peer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectedPeerSnapshot {
    /// Stable peer identifier.
    pub peer_id: PeerId,
    /// Advertised listener address when known, otherwise the transport endpoint
    /// (or an address with port zero when an inbound listener is unknown).
    pub address: SocketAddr,
}

impl PeerRegistry {
    /// Maximum number of unconnected candidate endpoints retained in the
    /// address book (C# `Peer.UnconnectedMax` = 1000).
    pub const UNCONNECTED_MAX: usize = 1000;
}

impl BlockRangeFetcher for Arc<PeerRegistry> {
    fn fetch_range(
        &self,
        assignment: BlockRangeAssignment,
    ) -> impl std::future::Future<Output = NetworkResult<BlockDownloadBatch>> + Send + 'static {
        let registry = Arc::clone(self);
        async move {
            let Some(handle) = registry.handle(assignment.peer_id) else {
                return Err(NetworkError::RemoteUnavailable {
                    peer_id: assignment.peer_id.to_string(),
                    detail: "peer is no longer connected".to_string(),
                });
            };
            handle.fetch_blocks_by_index(assignment.request).await
        }
    }
}

impl std::fmt::Debug for PeerRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock();
        f.debug_struct("PeerRegistry")
            .field("connected", &inner.peers.len())
            .field("max_connections", &self.max_connections)
            .field(
                "max_connections_per_address",
                &self.max_connections_per_address,
            )
            .finish()
    }
}

impl PeerRegistry {
    /// Build a registry from the connection limits in a
    /// [`ChannelsConfig`].
    pub fn from_config(config: &ChannelsConfig) -> Self {
        Self::with_limits(config.max_connections, config.max_connections_per_address)
    }

    /// Build a registry with explicit limits.
    pub fn with_limits(max_connections: usize, max_connections_per_address: usize) -> Self {
        Self {
            max_connections,
            max_connections_per_address,
            inner: parking_lot::Mutex::new(RegistryInner {
                peers: HashMap::new(),
                address_counts: HashMap::new(),
                unconnected: HashSet::new(),
            }),
        }
    }

    /// Admit a freshly connected peer, enforcing the connection caps
    /// of C# `Peer.OnTcpConnected`:
    ///
    /// - total connected peers `>= MaxConnections` → rejected,
    /// - connections from `remote_addr.ip()` already at
    ///   `MaxConnectionsPerAddress` → rejected.
    ///
    /// On admission the per-address count is incremented and the
    /// handle is stored; the caller is expected to abort the
    /// connection (drop the stream) when `false` is returned, exactly
    /// like C# replies `Tcp.Abort` without ever creating the
    /// `RemoteNode` actor.
    pub fn try_admit(
        &self,
        peer_id: PeerId,
        remote_addr: SocketAddr,
        handle: RemoteNodeHandle,
    ) -> bool {
        self.try_admit_with_direction(peer_id, remote_addr, handle, false)
    }

    /// Admit a peer while recording whether its transport was inbound.
    pub fn try_admit_with_direction(
        &self,
        peer_id: PeerId,
        remote_addr: SocketAddr,
        handle: RemoteNodeHandle,
        inbound: bool,
    ) -> bool {
        let mut inner = self.inner.lock();
        if inner.peers.len() >= self.max_connections {
            return false;
        }
        let count = inner
            .address_counts
            .get(&remote_addr.ip())
            .copied()
            .unwrap_or(0);
        if count >= self.max_connections_per_address {
            return false;
        }
        inner.address_counts.insert(remote_addr.ip(), count + 1);
        inner.peers.insert(
            peer_id,
            PeerEntry {
                handle,
                remote_addr,
                version_nonce: None,
                listener_addr: None,
                last_block_index: None,
                ready: false,
                inbound,
            },
        );
        true
    }

    /// Record the version-payload nonce for an admitted peer, applying
    /// the duplicate-connection filter of C#
    /// `LocalNode.AllowNewConnection`: returns `false` (and records
    /// nothing) when *another* connected peer with the same remote IP
    /// address already presented the same nonce, or when `peer_id` is
    /// no longer registered.
    pub fn record_version_nonce(&self, peer_id: PeerId, nonce: u32) -> bool {
        let mut inner = self.inner.lock();
        let Some(remote_ip) = inner.peers.get(&peer_id).map(|e| e.remote_addr.ip()) else {
            return false;
        };
        let duplicate = inner.peers.iter().any(|(other_id, other)| {
            *other_id != peer_id
                && other.remote_addr.ip() == remote_ip
                && other.version_nonce == Some(nonce)
        });
        if duplicate {
            return false;
        }
        if let Some(entry) = inner.peers.get_mut(&peer_id) {
            entry.version_nonce = Some(nonce);
        }
        true
    }

    /// Record a peer's advertised listener endpoint (remote IP + its
    /// `TcpServer` capability port), learned at handshake. A no-op for an
    /// unknown peer.
    pub fn record_listener_addr(&self, peer_id: PeerId, listener_addr: SocketAddr) {
        let mut inner = self.inner.lock();
        if let Some(entry) = inner.peers.get_mut(&peer_id) {
            entry.listener_addr = Some(listener_addr);
        }
    }

    /// Record the latest block height advertised by a peer.
    ///
    /// Called from the per-peer session when the `FullNode` capability is
    /// received and on every post-handshake ping/pong height refresh.
    pub fn record_block_height(&self, peer_id: PeerId, height: u32) {
        let mut inner = self.inner.lock();
        if let Some(entry) = inner.peers.get_mut(&peer_id) {
            entry.last_block_index = Some(height);
        }
    }

    /// Mark a peer eligible for data-plane work after its version/verack
    /// handshake and queued-message flush complete.
    pub(crate) fn mark_ready(&self, peer_id: PeerId) {
        let mut inner = self.inner.lock();
        if let Some(entry) = inner.peers.get_mut(&peer_id) {
            entry.ready = true;
        }
    }

    /// Snapshot of connected peers that can serve block downloads.
    ///
    /// Peers only appear after they have advertised a block height and completed
    /// the handshake. The list is deterministic by peer id so downloader tests
    /// and logs are stable.
    pub fn download_peers(&self) -> Vec<BlockDownloadPeer> {
        let inner = self.inner.lock();
        let mut peers = inner
            .peers
            .iter()
            .filter_map(|(peer_id, entry)| {
                if !entry.ready {
                    return None;
                }
                entry
                    .last_block_index
                    .map(|height| BlockDownloadPeer::new(*peer_id, height))
            })
            .collect::<Vec<_>>();
        peers.sort_by_key(|peer| peer.peer_id);
        peers
    }

    /// Distinct advertised listener endpoints of currently connected peers,
    /// excluding `exclude` (the requester) and capped at `limit`. Answers a
    /// peer's `GetAddr` (C# `OnGetAddrMessageReceived`: connected peers with
    /// `ListenerTcpPort > 0`, deduplicated by address).
    pub fn listener_addresses(&self, exclude: PeerId, limit: usize) -> Vec<SocketAddr> {
        let inner = self.inner.lock();
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for (peer_id, entry) in inner.peers.iter() {
            if *peer_id == exclude {
                continue;
            }
            if let Some(addr) = entry.listener_addr {
                if addr.port() > 0 && seen.insert(addr) {
                    out.push(addr);
                    if out.len() >= limit {
                        break;
                    }
                }
            }
        }
        out
    }

    /// Add candidate peer endpoints to the unconnected address book
    /// (C# `Peer.AddPeers`), skipping any endpoint that duplicates a
    /// currently connected peer's transport or advertised listener
    /// endpoint. The book is capped at [`Self::UNCONNECTED_MAX`]: once at
    /// capacity, C# stops taking new peers, so we do the same.
    ///
    /// Returns the number of endpoints newly inserted.
    pub fn add_unconnected(&self, endpoints: impl IntoIterator<Item = SocketAddr>) -> usize {
        let mut inner = self.inner.lock();
        // C# `AddPeers` gates on the whole batch: `if (UnconnectedPeers.Count
        // < UnconnectedMax)`. Preserve that (no partial fill past the cap).
        if inner.unconnected.len() >= Self::UNCONNECTED_MAX {
            return 0;
        }
        // Build the set of endpoints already represented by a live peer so a
        // candidate we are already connected to is never re-queued (C#
        // `AddPeers` filters against `ConnectedPeers.Values`).
        let mut connected: HashSet<SocketAddr> = HashSet::new();
        for entry in inner.peers.values() {
            connected.insert(entry.remote_addr);
            if let Some(addr) = entry.listener_addr {
                connected.insert(addr);
            }
        }
        let mut added = 0;
        for endpoint in endpoints {
            if inner.unconnected.len() >= Self::UNCONNECTED_MAX {
                break;
            }
            if connected.contains(&endpoint) {
                continue;
            }
            if inner.unconnected.insert(endpoint) {
                added += 1;
            }
        }
        added
    }

    /// Remove and return up to `count` unconnected candidate endpoints for
    /// dialing (C# `Peer.OnTimer`: `UnconnectedPeers.Sample(...)` followed by
    /// `Except`). Fewer than `count` are returned when the book holds fewer.
    pub fn take_unconnected(&self, count: usize) -> Vec<SocketAddr> {
        if count == 0 {
            return Vec::new();
        }
        let mut inner = self.inner.lock();
        let taken: Vec<SocketAddr> = inner.unconnected.iter().copied().take(count).collect();
        for endpoint in &taken {
            inner.unconnected.remove(endpoint);
        }
        taken
    }

    /// Number of unconnected candidate endpoints currently queued
    /// (C# `LocalNode.UnconnectedCount`).
    pub fn unconnected_len(&self) -> usize {
        self.inner.lock().unconnected.len()
    }

    /// Remove a peer, decrementing its per-address count
    /// (C# `Peer.OnTerminated`). Idempotent: removing an unknown peer
    /// is a no-op returning `false`.
    pub fn remove(&self, peer_id: PeerId) -> bool {
        let mut inner = self.inner.lock();
        let Some(entry) = inner.peers.remove(&peer_id) else {
            return false;
        };
        let ip = entry.remote_addr.ip();
        match inner.address_counts.get_mut(&ip) {
            Some(count) if *count > 1 => *count -= 1,
            Some(_) => {
                inner.address_counts.remove(&ip);
            }
            None => {}
        }
        true
    }

    /// Command handle for a connected peer, if registered.
    pub fn handle(&self, peer_id: PeerId) -> Option<RemoteNodeHandle> {
        self.inner
            .lock()
            .peers
            .get(&peer_id)
            .map(|e| e.handle.clone())
    }

    /// Snapshot of all connected peers' `(id, handle)` pairs, for
    /// broadcast fan-out and shutdown draining.
    pub fn handles(&self) -> Vec<(PeerId, RemoteNodeHandle)> {
        self.inner
            .lock()
            .peers
            .iter()
            .map(|(id, entry)| (*id, entry.handle.clone()))
            .collect()
    }

    /// Authoritative, deterministic snapshot of all connected peers.
    ///
    /// RPC and telemetry consumers should use this snapshot rather than
    /// folding the lossy lifecycle broadcast. The registry owns connection
    /// admission/removal, so the result cannot drift when a broadcast receiver
    /// lags.
    pub fn connected_snapshot(&self) -> Vec<ConnectedPeerSnapshot> {
        let inner = self.inner.lock();
        let mut peers: Vec<_> = inner
            .peers
            .iter()
            .map(|(peer_id, entry)| ConnectedPeerSnapshot {
                peer_id: *peer_id,
                address: entry.listener_addr.unwrap_or_else(|| {
                    if entry.inbound {
                        SocketAddr::new(entry.remote_addr.ip(), 0)
                    } else {
                        entry.remote_addr
                    }
                }),
            })
            .collect();
        peers.sort_unstable_by_key(|peer| peer.peer_id);
        peers
    }

    /// Number of currently connected peers
    /// (C# `LocalNode.ConnectedCount`).
    pub fn len(&self) -> usize {
        self.inner.lock().peers.len()
    }

    /// `true` when no peers are connected.
    pub fn is_empty(&self) -> bool {
        self.inner.lock().peers.is_empty()
    }
}

#[cfg(test)]
#[path = "../tests/peers/peer_registry.rs"]
mod tests;
