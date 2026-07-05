//! GetAddr peer-discovery loop coverage (fix-plan rank 16).
//!
//! Exercises the C# `Peer.OnTimer` + `LocalNode.NeedMorePeers` behavior
//! ported into the reth-style `LocalNodeService`:
//!
//! - when the connected count is below `MinDesiredConnections` and no
//!   candidate endpoints are queued, the node broadcasts `GetAddr` to its
//!   connected peers (C# `NeedMorePeers` → `BroadcastMessage(GetAddr)`);
//! - a *solicited* `Addr` reply is ingested into the unconnected address
//!   book (C# `OnAddrMessageReceived` → `Peer.AddPeers`);
//! - the next discovery tick dials a queued candidate (C# `OnTimer` samples
//!   `UnconnectedPeers` and calls `ConnectToPeer`).

use super::*;

use std::net::SocketAddr;

use neo_payloads::p2p_payloads::{AddrPayload, NetworkAddressWithTime};

/// Build a `LocalNodeService` with a fast discovery cadence so the tick
/// fires within the test budget instead of the 5 s production interval.
async fn start_local_node_fast_discovery(
    config: ChannelsConfig,
    discovery_interval: Duration,
) -> (NetworkHandle, broadcast::Receiver<NetworkEvent>, u16) {
    let settings = Arc::new(ProtocolSettings::default());
    let (service, handle) = LocalNodeService::with_config(settings, config);
    let service = service.with_discovery_interval(discovery_interval);
    tokio::spawn(service.run());
    let events = handle.subscribe();
    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");
    let port = handle.local_node_info().port();
    assert_ne!(port, 0);
    (handle, events, port)
}

/// Read frames until a `GetAddr` arrives (skipping the keepalive `Ping`
/// and any block-sync request the node may interleave).
async fn recv_getaddr(fake: &mut FakeFramed) {
    loop {
        let frame = recv_frame(fake).await.expect("expected a getaddr frame");
        if frame.command == MessageCommand::GetAddr {
            return;
        }
    }
}

fn addr_message(endpoints: &[SocketAddr]) -> Message {
    let entries: Vec<NetworkAddressWithTime> = endpoints
        .iter()
        .map(|ep| {
            NetworkAddressWithTime::new(
                0,
                ep.ip(),
                vec![NodeCapability::tcp_server(ep.port())],
            )
        })
        .collect();
    let payload = AddrPayload::create(entries);
    Message::create(MessageCommand::Addr, Some(&payload), false).expect("encode addr")
}

/// Full loop: below the desired minimum, the node solicits `GetAddr`; a
/// solicited `Addr` reply feeds the address book; the node then dials the
/// advertised endpoint.
#[tokio::test]
async fn below_min_desired_sends_getaddr_then_dials_addr_reply() {
    // A second listener stands in for the peer the `Addr` reply advertises;
    // the node under test must dial it after ingesting the reply.
    let advertised_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind advertised");
    let advertised_addr = advertised_listener.local_addr().expect("advertised addr");

    // min_desired = 2 so a single connected peer keeps the node hungry.
    let config = ChannelsConfig {
        min_desired_connections: 2,
        ..ChannelsConfig::default()
    };
    let (_handle, _events, port) =
        start_local_node_fast_discovery(config, Duration::from_millis(100)).await;

    // Fake peer dials in and completes the handshake, advertising its own
    // listener port (non-zero) so it is a normal full-node peer.
    let mut fake = fake_dial(port).await;
    let network = ProtocolSettings::default().network;
    complete_handshake(&mut fake, network, 0x1122_3344, 40333).await;

    // With one connection (< 2 desired) and an empty address book, the node
    // must broadcast GetAddr to us (C# NeedMorePeers).
    recv_getaddr(&mut fake).await;

    // Reply with an Addr advertising the second listener's endpoint. Because
    // we solicited it (we just received GetAddr), the node ingests it.
    fake.send(addr_message(&[advertised_addr]))
        .await
        .expect("send addr");

    // The next discovery tick dials the advertised endpoint: assert the
    // second listener accepts an inbound connection from the node.
    let accepted = tokio::time::timeout(TEST_TIMEOUT, advertised_listener.accept())
        .await
        .expect("node did not dial the advertised addr in time")
        .expect("accept advertised dial");
    // The node opened the connection (source port is ephemeral; we only need
    // to observe that a connection to the advertised endpoint occurred).
    drop(accepted);
}

/// An *unsolicited* `Addr` (no preceding `GetAddr`) is dropped, matching C#
/// `OnAddrMessageReceived`'s `if (!sent) return;` guard: the node must not
/// dial an endpoint it never asked for.
#[tokio::test]
async fn unsolicited_addr_is_ignored() {
    let advertised_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind advertised");
    let advertised_addr = advertised_listener.local_addr().expect("advertised addr");

    // min_desired = 1 so a single connected peer already satisfies the node
    // and no GetAddr is ever sent — any Addr we push is therefore unsolicited.
    let config = ChannelsConfig {
        min_desired_connections: 1,
        ..ChannelsConfig::default()
    };
    let (_handle, _events, port) =
        start_local_node_fast_discovery(config, Duration::from_millis(100)).await;

    let mut fake = fake_dial(port).await;
    let network = ProtocolSettings::default().network;
    complete_handshake(&mut fake, network, 0x5566_7788, 40334).await;

    // Push an unsolicited Addr. The node never sent GetAddr (it is at its
    // desired minimum), so this must be ignored and no dial should follow.
    fake.send(addr_message(&[advertised_addr]))
        .await
        .expect("send addr");

    // No inbound connection should reach the advertised listener.
    let accepted = tokio::time::timeout(Duration::from_millis(500), advertised_listener.accept())
        .await;
    assert!(
        accepted.is_err(),
        "an unsolicited addr must not cause the node to dial"
    );
}
