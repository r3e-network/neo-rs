use super::*;
use crate::types::RpcPeers;

#[tokio::test(flavor = "multi_thread")]
async fn get_peers_serves_empty_arrays_without_peers() {
    // With no peer lifecycle events folded, all three arrays are
    // empty. `unconnected` stays empty by design (the reth-style
    // network service keeps no unconnected address book) and `bad` is
    // always empty, matching C# v3.10.1.
    let system = crate::server::test_support::test_system(ProtocolSettings::default());

    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let peers_handler = find_handler(&handlers, "getpeers");

    let result = (peers_handler.callback())(&server, &[]).expect("get peers");
    let unconnected = result
        .get("unconnected")
        .and_then(|v| v.as_array())
        .expect("unconnected array");
    assert!(unconnected.is_empty());

    let bad = result
        .get("bad")
        .and_then(|v| v.as_array())
        .expect("bad array");
    assert!(bad.is_empty());

    let connected = result
        .get("connected")
        .and_then(|v| v.as_array())
        .expect("connected array");
    assert!(connected.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_peers_folds_connect_and_disconnect_events() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let peers_handler = find_handler(&handlers, "getpeers");
    let count_handler = find_handler(&handlers, "getconnectioncount");

    let network = system.network();
    let events = network.event_sender();

    // Outbound-style peer: the dial path publishes the lifecycle
    // event carrying the dialed endpoint (the peer's listener — the
    // `Remote.Address` / `ListenerTcpPort` pair C# reports).
    events
        .send(neo_network::NetworkEvent::PeerConnected {
            peer_id: "peer:11".to_string(),
            address: Some("10.1.2.3:20333".parse().expect("addr")),
        })
        .expect("publish connect");
    // Inbound-style peer: the accept loop publishes the accepted
    // connection's source endpoint (remote IP + ephemeral source
    // port; C# would report the listener port advertised in the
    // peer's version payload, which the Rust per-peer service does
    // not capture yet).
    events
        .send(neo_network::NetworkEvent::PeerConnected {
            peer_id: "peer:12".to_string(),
            address: Some("198.51.100.7:54321".parse().expect("addr")),
        })
        .expect("publish connect");
    // Address-less peer: folds into the connection count but is
    // omitted from the connected array (no address to report).
    events
        .send(neo_network::NetworkEvent::PeerConnected {
            peer_id: "peer:13".to_string(),
            address: None,
        })
        .expect("publish connect");

    let result = (peers_handler.callback())(&server, &[]).expect("get peers");
    let connected = result
        .get("connected")
        .and_then(|v| v.as_array())
        .expect("connected array");
    assert_eq!(connected.len(), 2);
    // Deterministic peer-id order: "peer:11" < "peer:12".
    assert_eq!(
        connected[0].get("address").and_then(Value::as_str),
        Some("10.1.2.3")
    );
    // C# emits the port as a JSON number, not a string.
    assert_eq!(
        connected[0].get("port").and_then(Value::as_u64),
        Some(20333)
    );
    // The inbound peer appears with its remote address.
    assert_eq!(
        connected[1].get("address").and_then(Value::as_str),
        Some("198.51.100.7")
    );
    assert_eq!(
        connected[1].get("port").and_then(Value::as_u64),
        Some(54321)
    );
    let count = (count_handler.callback())(&server, &[]).expect("count");
    assert_eq!(count.as_u64(), Some(3));

    // The C#-shaped payload roundtrips through the client model.
    let parsed = RpcPeers::from_json(&parse_object(&result)).expect("parse peers");
    assert!(parsed.unconnected.is_empty());
    assert!(parsed.bad.is_empty());
    assert_eq!(parsed.connected.len(), 2);
    assert_eq!(parsed.connected[0].address, "10.1.2.3");
    assert_eq!(parsed.connected[0].port, 20333);
    assert_eq!(parsed.connected[1].address, "198.51.100.7");
    assert_eq!(parsed.connected[1].port, 54321);

    // Disconnects remove the peers from the folded view.
    events
        .send(neo_network::NetworkEvent::PeerDisconnected {
            peer_id: "peer:11".to_string(),
        })
        .expect("publish disconnect");
    events
        .send(neo_network::NetworkEvent::PeerDisconnected {
            peer_id: "peer:12".to_string(),
        })
        .expect("publish disconnect");
    let result = (peers_handler.callback())(&server, &[]).expect("get peers");
    let connected = result
        .get("connected")
        .and_then(|v| v.as_array())
        .expect("connected array");
    assert!(connected.is_empty());
    let count = (count_handler.callback())(&server, &[]).expect("count");
    assert_eq!(count.as_u64(), Some(1));

    // A stale address record for an already-disconnected peer must
    // not resurrect it in either view.
    network.record_peer_address("peer:11", "10.1.2.3:20333".parse().expect("addr"));
    let result = (peers_handler.callback())(&server, &[]).expect("get peers");
    let connected = result
        .get("connected")
        .and_then(|v| v.as_array())
        .expect("connected array");
    assert!(connected.is_empty());
    let count = (count_handler.callback())(&server, &[]).expect("count");
    assert_eq!(count.as_u64(), Some(1));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_peers_empty_when_no_queue() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let peers_handler = find_handler(&handlers, "getpeers");

    let result = (peers_handler.callback())(&server, &[]).expect("get peers");
    let unconnected = result
        .get("unconnected")
        .and_then(|v| v.as_array())
        .expect("unconnected array");
    assert!(unconnected.is_empty());

    let bad = result
        .get("bad")
        .and_then(|v| v.as_array())
        .expect("bad array");
    assert!(bad.is_empty());

    let connected = result
        .get("connected")
        .and_then(|v| v.as_array())
        .expect("connected array");
    assert!(connected.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_peers_roundtrips_into_client_model() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let peers_handler = find_handler(&handlers, "getpeers");

    let result = (peers_handler.callback())(&server, &[]).expect("get peers");
    let parsed = RpcPeers::from_json(&parse_object(&result)).expect("parse peers");
    assert!(parsed.unconnected.is_empty());
    assert!(parsed.connected.is_empty());
}
