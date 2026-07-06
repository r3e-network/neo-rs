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
fn listener_addresses_dedup_exclude_and_skip_unset() {
    let registry = PeerRegistry::with_limits(10, 10);
    let (a, _) = admit(&registry, "10.0.0.1:5001");
    let (b, _) = admit(&registry, "10.0.0.2:5002");
    let (c, _) = admit(&registry, "10.0.0.3:5003");

    // a + b advertise listeners; c advertises none (stays unset).
    registry.record_listener_addr(a, addr("10.0.0.1:20333"));
    registry.record_listener_addr(b, addr("10.0.0.2:20333"));
    // A duplicate listener endpoint from another peer is deduplicated.
    let (d, _) = admit(&registry, "10.0.0.2:5004");
    registry.record_listener_addr(d, addr("10.0.0.2:20333"));

    // Excluding `a` yields b's listener (and d's, deduped to one).
    let mut got = registry.listener_addresses(a, 100);
    got.sort();
    assert_eq!(got, vec![addr("10.0.0.2:20333")]);

    // Excluding `c` (no listener) yields both a and b, c absent.
    let mut got = registry.listener_addresses(c, 100);
    got.sort();
    assert_eq!(got, vec![addr("10.0.0.1:20333"), addr("10.0.0.2:20333")]);

    // The limit caps the result.
    assert_eq!(registry.listener_addresses(c, 1).len(), 1);
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
fn add_unconnected_dedups_and_skips_connected_endpoints() {
    let registry = PeerRegistry::with_limits(100, 10);
    // A connected peer whose listener endpoint must not be re-queued.
    let (peer, _) = admit(&registry, "10.0.0.1:5001");
    registry.record_listener_addr(peer, addr("10.0.0.1:20333"));

    let added = registry.add_unconnected([
        addr("10.0.0.2:20333"),
        addr("10.0.0.3:20333"),
        addr("10.0.0.2:20333"), // duplicate within the batch
        addr("10.0.0.1:20333"), // already connected (listener) => skipped
        addr("10.0.0.1:5001"),  // already connected (transport) => skipped
    ]);
    assert_eq!(added, 2, "only two distinct, non-connected endpoints");
    assert_eq!(registry.unconnected_len(), 2);

    // Re-adding an already-queued endpoint is a no-op.
    assert_eq!(registry.add_unconnected([addr("10.0.0.2:20333")]), 0);
    assert_eq!(registry.unconnected_len(), 2);
}

#[test]
fn take_unconnected_drains_up_to_count() {
    let registry = PeerRegistry::with_limits(100, 10);
    registry.add_unconnected([
        addr("10.0.0.2:20333"),
        addr("10.0.0.3:20333"),
        addr("10.0.0.4:20333"),
    ]);
    assert_eq!(registry.unconnected_len(), 3);

    let taken = registry.take_unconnected(2);
    assert_eq!(taken.len(), 2);
    assert_eq!(registry.unconnected_len(), 1, "taken endpoints are removed");

    // take(0) is a no-op; taking more than remain returns just the remainder.
    assert!(registry.take_unconnected(0).is_empty());
    assert_eq!(registry.take_unconnected(10).len(), 1);
    assert_eq!(registry.unconnected_len(), 0);
}

#[test]
fn add_unconnected_respects_the_unconnected_cap() {
    let registry = PeerRegistry::with_limits(100, 10);
    // Fill to the cap.
    let batch: Vec<SocketAddr> = (0..PeerRegistry::UNCONNECTED_MAX)
        .map(|i| addr(&format!("10.1.{}.{}:20333", i / 256, i % 256)))
        .collect();
    let added = registry.add_unconnected(batch);
    assert_eq!(added, PeerRegistry::UNCONNECTED_MAX);
    assert_eq!(registry.unconnected_len(), PeerRegistry::UNCONNECTED_MAX);

    // At capacity, further additions are rejected wholesale (C# `AddPeers`).
    assert_eq!(registry.add_unconnected([addr("10.9.9.9:20333")]), 0);
    assert_eq!(registry.unconnected_len(), PeerRegistry::UNCONNECTED_MAX);
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

#[test]
fn download_peers_are_height_snapshots_for_advertising_full_nodes() {
    let registry = PeerRegistry::with_limits(100, 10);
    let (first, _) = admit(&registry, "10.0.0.1:1001");
    let (second, _) = admit(&registry, "10.0.0.2:1002");

    assert!(
        registry.download_peers().is_empty(),
        "peers do not become download candidates until they advertise a height"
    );

    registry.record_block_height(second, 42);
    registry.record_block_height(first, 7);
    let peers = registry.download_peers();

    assert_eq!(peers.len(), 2);
    assert!(
        peers
            .iter()
            .any(|peer| peer.peer_id == first && peer.height == 7)
    );
    assert!(
        peers
            .iter()
            .any(|peer| peer.peer_id == second && peer.height == 42)
    );
    assert!(
        peers
            .windows(2)
            .all(|window| window[0].peer_id < window[1].peer_id),
        "snapshot order is deterministic by peer id"
    );

    assert!(registry.remove(first));
    let peers = registry.download_peers();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].peer_id, second);
    assert_eq!(peers[0].height, 42);
}
