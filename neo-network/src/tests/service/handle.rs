use super::*;
use crate::remote_node::RemoteNodeHandle;

fn test_handle() -> (
    NetworkHandle,
    mpsc::Receiver<NetworkCommand>,
    broadcast::Sender<NetworkEvent>,
) {
    NetworkHandle::channel(8, 32)
}

fn addr(s: &str) -> SocketAddr {
    s.parse().expect("socket address")
}

fn transaction_with_nonce(nonce: u32) -> Transaction {
    let mut transaction = Transaction::new();
    transaction.set_nonce(nonce);
    transaction
}

#[test]
fn try_broadcast_transaction_splits_full_and_closed_errors() {
    let (handle, cmd_rx, _events) = NetworkHandle::channel(1, 32);

    assert!(
        handle
            .try_broadcast_transaction(transaction_with_nonce(1))
            .is_ok()
    );

    let full_error = handle
        .try_broadcast_transaction(transaction_with_nonce(2))
        .expect_err("second non-blocking send should see full command queue");
    assert!(matches!(full_error, NetworkError::ChannelFull));

    drop(cmd_rx);
    let closed_error = handle
        .try_broadcast_transaction(transaction_with_nonce(3))
        .expect_err("send after receiver drop should see closed command channel");
    assert!(matches!(closed_error, NetworkError::LocalShuttingDown));
}

#[test]
fn folds_peer_connected_and_disconnected_events() {
    let (handle, _cmd_rx, events) = test_handle();
    assert_eq!(handle.local_node_info().connected_peers_count(), 0);
    assert!(handle.local_node_info().connected_peers().is_empty());

    events
        .send(NetworkEvent::PeerConnected {
            peer_id: "peer:1".to_string(),
            address: None,
        })
        .expect("publish");
    events
        .send(NetworkEvent::PeerConnected {
            peer_id: "peer:2".to_string(),
            address: None,
        })
        .expect("publish");

    let info = handle.local_node_info();
    assert_eq!(info.connected_peers_count(), 2);
    // Events without an address fold as address-less peers.
    assert!(
        info.connected_peers()
            .iter()
            .all(|peer| peer.address.is_none())
    );

    events
        .send(NetworkEvent::PeerDisconnected {
            peer_id: "peer:1".to_string(),
        })
        .expect("publish");

    let info = handle.local_node_info();
    assert_eq!(info.connected_peers_count(), 1);
    assert_eq!(info.connected_peers()[0].peer_id, "peer:2");
}

#[test]
fn inbound_peer_connected_event_folds_with_address() {
    // The accept loop publishes the accepted connection's source
    // endpoint in the event itself, so inbound peers fold with
    // their address attached - no out-of-band recording involved.
    let (handle, _cmd_rx, events) = test_handle();
    let remote = addr("198.51.100.23:54321");

    events
        .send(NetworkEvent::PeerConnected {
            peer_id: "peer:8".to_string(),
            address: Some(remote),
        })
        .expect("publish");

    let info = handle.local_node_info();
    assert_eq!(info.connected_peers_count(), 1);
    assert_eq!(
        info.connected_peers()[0],
        ConnectedPeer {
            peer_id: "peer:8".to_string(),
            address: Some(remote),
        }
    );
}

#[test]
fn duplicate_peer_connected_events_fold_once() {
    let (handle, _cmd_rx, events) = test_handle();
    for _ in 0..3 {
        events
            .send(NetworkEvent::PeerConnected {
                peer_id: "peer:1".to_string(),
                address: None,
            })
            .expect("publish");
    }
    assert_eq!(handle.local_node_info().connected_peers_count(), 1);
}

#[test]
fn duplicate_peer_connected_without_address_keeps_known_address() {
    let (handle, _cmd_rx, events) = test_handle();
    let remote = addr("192.168.1.4:10333");

    events
        .send(NetworkEvent::PeerConnected {
            peer_id: "peer:3".to_string(),
            address: Some(remote),
        })
        .expect("publish");
    // A duplicate lifecycle event with no address must not erase
    // the address learned from the first event.
    events
        .send(NetworkEvent::PeerConnected {
            peer_id: "peer:3".to_string(),
            address: None,
        })
        .expect("publish");

    let info = handle.local_node_info();
    assert_eq!(info.connected_peers().len(), 1);
    assert_eq!(info.connected_peers()[0].address, Some(remote));
}

#[test]
fn record_peer_address_attaches_address_until_disconnect() {
    let (handle, _cmd_rx, events) = test_handle();
    let remote = addr("10.0.0.9:20333");

    // Out-of-band recording upgrades an address-less entry created
    // by the peer's own lifecycle event.
    events
        .send(NetworkEvent::PeerConnected {
            peer_id: "peer:7".to_string(),
            address: None,
        })
        .expect("publish");
    handle.record_peer_address("peer:7", remote);

    let info = handle.local_node_info();
    assert_eq!(info.connected_peers_count(), 1);
    assert_eq!(
        info.connected_peers()[0],
        ConnectedPeer {
            peer_id: "peer:7".to_string(),
            address: Some(remote),
        }
    );

    events
        .send(NetworkEvent::PeerDisconnected {
            peer_id: "peer:7".to_string(),
        })
        .expect("publish");
    assert_eq!(handle.local_node_info().connected_peers_count(), 0);
}

#[test]
fn record_peer_address_does_not_resurrect_after_folded_disconnect() {
    // Phantom-resurrect race: the peer connected and disconnected,
    // both events already folded; a straggling address record for
    // the stale peer id must not re-create the entry (nothing
    // would ever remove it again).
    let (handle, _cmd_rx, events) = test_handle();
    let remote = addr("10.0.0.9:20333");

    events
        .send(NetworkEvent::PeerConnected {
            peer_id: "peer:7".to_string(),
            address: Some(remote),
        })
        .expect("publish");
    events
        .send(NetworkEvent::PeerDisconnected {
            peer_id: "peer:7".to_string(),
        })
        .expect("publish");
    // Fold both events before the stale record arrives.
    assert_eq!(handle.local_node_info().connected_peers_count(), 0);

    handle.record_peer_address("peer:7", remote);
    assert_eq!(handle.local_node_info().connected_peers_count(), 0);
    assert!(handle.local_node_info().connected_peers().is_empty());
}

#[test]
fn record_peer_address_does_not_resurrect_with_pending_disconnect() {
    // Same race, other interleaving: the disconnect event is still
    // queued (not yet folded) when the stale record lands. The
    // record folds pending events first, so the disconnect wins.
    let (handle, _cmd_rx, events) = test_handle();
    let remote = addr("10.0.0.9:20333");

    events
        .send(NetworkEvent::PeerConnected {
            peer_id: "peer:7".to_string(),
            address: Some(remote),
        })
        .expect("publish");
    events
        .send(NetworkEvent::PeerDisconnected {
            peer_id: "peer:7".to_string(),
        })
        .expect("publish");

    handle.record_peer_address("peer:7", remote);
    assert_eq!(handle.local_node_info().connected_peers_count(), 0);
    assert!(handle.local_node_info().connected_peers().is_empty());
}

#[test]
fn record_peer_address_for_unknown_peer_is_a_no_op() {
    let (handle, _cmd_rx, _events) = test_handle();
    handle.record_peer_address("peer:404", addr("10.0.0.1:20333"));
    assert_eq!(handle.local_node_info().connected_peers_count(), 0);
}

#[test]
fn connected_peers_snapshot_is_ordered_by_peer_id() {
    let (handle, _cmd_rx, events) = test_handle();
    for id in ["peer:9", "peer:1", "peer:5"] {
        events
            .send(NetworkEvent::PeerConnected {
                peer_id: id.to_string(),
                address: None,
            })
            .expect("publish");
    }
    let info = handle.local_node_info();
    let ids: Vec<&str> = info
        .connected_peers()
        .iter()
        .map(|peer| peer.peer_id.as_str())
        .collect();
    assert_eq!(ids, vec!["peer:1", "peer:5", "peer:9"]);
}

#[test]
fn live_handle_reads_authoritative_registry_without_lifecycle_events() {
    let (cmd_tx, _cmd_rx) = mpsc::channel(8);
    let (event_tx, _event_rx) = broadcast::channel(8);
    let registry = Arc::new(PeerRegistry::with_limits(8, 8));
    let peer_id = PeerId::from_raw(12);
    let remote = addr("198.51.100.9:4000");
    let (peer_cmd_tx, _peer_cmd_rx) = mpsc::channel(1);
    assert!(registry.try_admit(
        peer_id,
        remote,
        RemoteNodeHandle::from_parts(peer_cmd_tx, peer_id, remote),
    ));
    let listener = addr("198.51.100.9:20333");
    registry.record_listener_addr(peer_id, listener);

    let handle =
        NetworkHandle::from_parts_with_registry(cmd_tx, event_tx, Some(Arc::clone(&registry)));
    let info = handle.local_node_info();
    assert_eq!(
        info.connected_peers(),
        &[ConnectedPeer {
            peer_id: peer_id.to_string(),
            address: Some(listener),
        }]
    );

    let upgraded = addr("198.51.100.9:30333");
    handle.record_peer_address(peer_id.to_string(), upgraded);
    assert_eq!(
        handle.local_node_info().connected_peers()[0].address,
        Some(upgraded)
    );

    assert!(registry.remove(peer_id));
    assert!(handle.local_node_info().connected_peers().is_empty());
}

#[tokio::test]
async fn connect_peer_folds_dialed_address_from_event() {
    let (handle, mut cmd_rx, events) = test_handle();

    // Stand-in for the service's `handle_connect_peer`: publish
    // the lifecycle event carrying the dialed address, then
    // resolve the dial reply - the same order `LocalNodeService`
    // uses.
    tokio::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            if let NetworkCommand::ConnectPeer { addr, reply } = cmd {
                let peer_id = PeerId::from_raw(42);
                let _ = events.send(NetworkEvent::PeerConnected {
                    peer_id: peer_id.to_string(),
                    address: Some(addr),
                });
                let _ = reply.send(Ok(peer_id));
            }
        }
    });

    let remote = addr("127.0.0.1:20333");
    let peer_id = handle.connect_peer(remote).await.expect("connect");
    assert_eq!(peer_id, PeerId::from_raw(42));

    let info = handle.local_node_info();
    assert_eq!(info.connected_peers_count(), 1);
    assert_eq!(info.connected_peers()[0].peer_id, peer_id.to_string());
    assert_eq!(info.connected_peers()[0].address, Some(remote));
}
