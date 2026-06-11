//! Shared connected-peer registry with admission control.
//!
//! The single source of truth for "who is connected", shared by the
//! [`crate::local_node::LocalNodeService`] command loop (dial path),
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

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

use neo_p2p::ChannelsConfig;

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
}

/// Interior, lock-guarded state.
struct RegistryInner {
    peers: HashMap<PeerId, PeerEntry>,
    /// Live connection count per remote IP address
    /// (C# `Peer.ConnectedAddresses`).
    address_counts: HashMap<IpAddr, usize>,
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
        self.inner.lock().peers.get(&peer_id).map(|e| e.handle.clone())
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
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn test_handle(peer_id: PeerId, remote_addr: SocketAddr) -> RemoteNodeHandle {
        let (cmd_tx, cmd_rx) = mpsc::channel(1);
        // The receiver half is intentionally leaked-by-drop: registry
        // unit tests only exercise bookkeeping, never the channel.
        drop(cmd_rx);
        RemoteNodeHandle::from_parts(cmd_tx, peer_id, remote_addr)
    }

    fn addr(s: &str) -> SocketAddr {
        s.parse().expect("socket address")
    }

    fn admit(registry: &PeerRegistry, peer: &str) -> (PeerId, SocketAddr) {
        let peer_id = PeerId::new();
        let remote = addr(peer);
        assert!(registry.try_admit(peer_id, remote, test_handle(peer_id, remote)));
        (peer_id, remote)
    }

    #[test]
    fn admits_until_total_cap_and_recovers_on_remove() {
        let registry = PeerRegistry::with_limits(2, 10);
        let (first, _) = admit(&registry, "10.0.0.1:1001");
        admit(&registry, "10.0.0.2:1002");
        assert_eq!(registry.len(), 2);

        let rejected = PeerId::new();
        let rejected_addr = addr("10.0.0.3:1003");
        assert!(!registry.try_admit(
            rejected,
            rejected_addr,
            test_handle(rejected, rejected_addr)
        ));
        assert_eq!(registry.len(), 2);

        assert!(registry.remove(first));
        admit(&registry, "10.0.0.3:1003");
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn per_address_cap_counts_by_ip_not_port() {
        let registry = PeerRegistry::with_limits(100, 2);
        admit(&registry, "10.0.0.1:1001");
        admit(&registry, "10.0.0.1:1002");

        let rejected = PeerId::new();
        let rejected_addr = addr("10.0.0.1:1003");
        assert!(!registry.try_admit(
            rejected,
            rejected_addr,
            test_handle(rejected, rejected_addr)
        ));
        // A different IP is still admissible.
        admit(&registry, "10.0.0.2:1001");
    }

    #[test]
    fn remove_decrements_per_address_count() {
        let registry = PeerRegistry::with_limits(100, 1);
        let (peer, _) = admit(&registry, "10.0.0.1:1001");
        assert!(registry.remove(peer));
        assert!(!registry.remove(peer), "second remove is a no-op");
        // Slot freed: same IP admissible again.
        admit(&registry, "10.0.0.1:1002");
    }

    #[test]
    fn duplicate_version_nonce_from_same_ip_is_rejected() {
        let registry = PeerRegistry::with_limits(100, 10);
        let (first, _) = admit(&registry, "10.0.0.1:1001");
        let (second, _) = admit(&registry, "10.0.0.1:1002");
        let (other_ip, _) = admit(&registry, "10.0.0.2:1001");

        assert!(registry.record_version_nonce(first, 7));
        assert!(
            !registry.record_version_nonce(second, 7),
            "same IP + same nonce duplicates the first connection"
        );
        assert!(
            registry.record_version_nonce(second, 8),
            "same IP with a different nonce is a distinct node"
        );
        assert!(
            registry.record_version_nonce(other_ip, 7),
            "same nonce from a different IP is allowed (C# filters by address AND nonce)"
        );
    }

    #[test]
    fn record_version_nonce_for_unregistered_peer_fails() {
        let registry = PeerRegistry::with_limits(100, 10);
        assert!(!registry.record_version_nonce(PeerId::new(), 1));
    }

    #[test]
    fn handles_snapshot_and_lookup() {
        let registry = PeerRegistry::with_limits(100, 10);
        let (peer, _) = admit(&registry, "10.0.0.1:1001");
        assert!(registry.handle(peer).is_some());
        assert!(registry.handle(PeerId::new()).is_none());
        assert_eq!(registry.handles().len(), 1);
        assert!(!registry.is_empty());
    }
}
