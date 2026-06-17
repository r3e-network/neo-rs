//! Integration tests for the P2P version/verack handshake and the
//! per-connection lifecycle, driven over real TCP with a scripted
//! fake peer speaking the `neo-network::wire` frame format.

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_util::codec::Framed;
use tokio_util::sync::CancellationToken;

use neo_config::ProtocolSettings;
use neo_network::wire::{Message, MessageCodec};
use neo_network::{
    ChannelsConfig, ConnectionTimeouts, InboundInventory, LocalIdentity, LocalNodeService,
    NetworkEvent, NetworkHandle, PeerId, PeerRegistry, RemoteNodeService, RemoteNodeState,
};
use neo_network::MessageCommand;
use neo_payloads::p2p_payloads::{
    GetBlockByIndexPayload, InvPayload, NodeCapability, PingPayload, VersionPayload,
};
use neo_primitives::{InventoryType, UInt256};

/// Generous upper bound for every await in these tests; real
/// exchanges complete in milliseconds.
const TEST_TIMEOUT: Duration = Duration::from_secs(10);

type FakeFramed = Framed<TcpStream, MessageCodec>;

async fn fake_dial(port: u16) -> FakeFramed {
    let stream = tokio::time::timeout(TEST_TIMEOUT, TcpStream::connect(("127.0.0.1", port)))
        .await
        .expect("dial timed out")
        .expect("dial failed");
    Framed::new(stream, MessageCodec::new())
}

/// Read the next frame, panicking on timeout. Returns `None` when the
/// remote closed the connection.
async fn recv_frame(framed: &mut FakeFramed) -> Option<Message> {
    match tokio::time::timeout(TEST_TIMEOUT, framed.next()).await {
        Ok(Some(Ok(message))) => Some(message),
        Ok(Some(Err(err))) => panic!("frame decode failed: {err}"),
        Ok(None) => None,
        Err(_) => panic!("timed out waiting for a frame"),
    }
}

/// Assert the connection is closed from the fake peer's perspective:
/// the next read yields EOF (or a reset, which dropping an aborted
/// socket may surface as an error).
async fn expect_closed(framed: &mut FakeFramed) {
    match tokio::time::timeout(TEST_TIMEOUT, framed.next()).await {
        Ok(None) | Ok(Some(Err(_))) => {}
        Ok(Some(Ok(message))) => panic!(
            "expected the connection to be closed, received {:?}",
            message.command
        ),
        Err(_) => panic!("timed out waiting for the connection to close"),
    }
}

fn version_message(network: u32, nonce: u32, listener_port: u16) -> Message {
    let mut capabilities = vec![NodeCapability::full_node(0)];
    if listener_port > 0 {
        capabilities.push(NodeCapability::tcp_server(listener_port));
    }
    let payload = VersionPayload::create(
        network,
        nonce,
        "/fake-peer:0.0.1/".to_string(),
        capabilities,
    );
    Message::create(MessageCommand::Version, Some(&payload), false).expect("encode version")
}

fn verack_message() -> Message {
    Message::from_payload_bytes(MessageCommand::Verack, Vec::new(), false).expect("encode verack")
}

fn decode_version(message: &Message) -> VersionPayload {
    assert_eq!(message.command, MessageCommand::Version);
    let mut reader = neo_io::MemoryReader::new(&message.payload_raw);
    <VersionPayload as neo_io::Serializable>::deserialize(&mut reader).expect("decode version")
}

/// Await the next event matching `pred`, panicking on timeout.
async fn await_event<F>(events: &mut broadcast::Receiver<NetworkEvent>, mut pred: F) -> NetworkEvent
where
    F: FnMut(&NetworkEvent) -> bool,
{
    loop {
        let event = tokio::time::timeout(TEST_TIMEOUT, events.recv())
            .await
            .expect("timed out waiting for a network event")
            .expect("event channel open");
        if pred(&event) {
            return event;
        }
    }
}

/// Poll `handle.local_node_info()` until `pred` holds, panicking on
/// timeout. Used for assertions on the handle-side `getpeers` fold.
async fn await_info<F>(handle: &NetworkHandle, mut pred: F)
where
    F: FnMut(&neo_network::handle::LocalNodeInfo) -> bool,
{
    let deadline = tokio::time::Instant::now() + TEST_TIMEOUT;
    loop {
        if pred(&handle.local_node_info()) {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for the handle-side peer view to converge"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Start a `LocalNodeService` bound to an ephemeral port, returning
/// `(handle, events, listen_port)`. The events receiver is subscribed
/// before the listener starts so no lifecycle event can be missed.
async fn start_local_node(
    config: ChannelsConfig,
) -> (NetworkHandle, broadcast::Receiver<NetworkEvent>, u16) {
    let settings = Arc::new(ProtocolSettings::default());
    let (service, handle) = LocalNodeService::with_config(settings, config);
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

async fn start_local_node_with_seeded_height(
    config: ChannelsConfig,
    height: u32,
) -> (NetworkHandle, broadcast::Receiver<NetworkEvent>, u16) {
    let settings = Arc::new(ProtocolSettings::default());
    let (service, handle) = LocalNodeService::with_config(settings, config);
    tokio::spawn(service.run());
    let events = handle.subscribe();
    handle
        .set_block_height(height)
        .await
        .expect("seed local height before listener starts");
    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");
    let port = handle.local_node_info().port();
    assert_ne!(port, 0);
    (handle, events, port)
}

/// Drive the fake-peer side of a full handshake: read the node's
/// version, reply with our own, read the verack, reply with verack.
/// Returns the node's version payload.
async fn complete_handshake(
    framed: &mut FakeFramed,
    network: u32,
    nonce: u32,
    listener_port: u16,
) -> VersionPayload {
    let node_version = recv_frame(framed).await.expect("node version");
    let node_version = decode_version(&node_version);
    framed
        .send(version_message(network, nonce, listener_port))
        .await
        .expect("send version");
    let verack = recv_frame(framed).await.expect("verack");
    assert_eq!(verack.command, MessageCommand::Verack);
    framed.send(verack_message()).await.expect("send verack");
    node_version
}

// ---------------------------------------------------------------------------
// Handshake + address upgrade + EOF lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn inbound_handshake_upgrades_address_and_eof_disconnects() {
    let (handle, mut events, port) = start_local_node(ChannelsConfig::default()).await;
    let local = handle.local_node_info();
    let network = ProtocolSettings::default().network;

    let mut fake = fake_dial(port).await;

    // The accept path publishes the C#-faithful unknown listener form
    // first: (remote_ip, 0).
    let connected = await_event(&mut events, |e| {
        matches!(e, NetworkEvent::PeerConnected { .. })
    })
    .await;
    let NetworkEvent::PeerConnected { peer_id, address } = connected else {
        unreachable!()
    };
    assert_eq!(address.expect("address").port(), 0);

    // The node sends its version immediately (C# StartProtocol); its
    // identity fields must match what the RPC layer reports.
    let node_version = complete_handshake(&mut fake, network, 0xfa4e_0001, 20333).await;
    assert_eq!(node_version.network, network);
    assert_eq!(node_version.nonce, local.nonce);
    assert_eq!(node_version.user_agent, local.user_agent);

    // After the version exchange the peer's advertised listener port
    // replaces the unknown form: C# `LocalNode.AllowNewConnection`
    // updates the connected endpoint to `node.Listener`.
    let upgraded = await_event(&mut events, |e| {
        matches!(
            e,
            NetworkEvent::PeerConnected { peer_id: p, address: Some(addr) }
                if *p == peer_id && addr.port() == 20333
        )
    })
    .await;
    let NetworkEvent::PeerConnected {
        address: Some(upgraded_addr),
        ..
    } = upgraded
    else {
        unreachable!()
    };
    assert_eq!(upgraded_addr.ip().to_string(), "127.0.0.1");

    // The `getpeers` view (handle-side fold) serves the upgraded
    // endpoint.
    await_info(&handle, |info| {
        info.connected_peers()
            .iter()
            .any(|p| p.peer_id == peer_id && p.address.map(|a| a.port()) == Some(20333))
    })
    .await;

    // EOF: closing the fake peer must publish `PeerDisconnected` and
    // remove the peer from the connected view (the
    // inbound-peers-persist-forever blocker).
    drop(fake);
    await_event(
        &mut events,
        |e| matches!(e, NetworkEvent::PeerDisconnected { peer_id: p } if *p == peer_id),
    )
    .await;
    await_info(&handle, |info| info.connected_peers_count() == 0).await;

    handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn outbound_dial_sends_version_and_completes_handshake() {
    let remote_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let remote_addr = remote_listener.local_addr().expect("addr");
    let network = ProtocolSettings::default().network;

    let (handle, mut events, _port) = start_local_node(ChannelsConfig::default()).await;
    let local = handle.local_node_info();

    let accept_task = tokio::spawn(async move {
        let (stream, _) = remote_listener.accept().await.expect("accept");
        Framed::new(stream, MessageCodec::new())
    });

    let peer_id = handle.connect_peer(remote_addr).await.expect("dial");
    let mut fake = tokio::time::timeout(TEST_TIMEOUT, accept_task)
        .await
        .expect("accept timed out")
        .expect("accept task");

    // The dial path sends version on connect; the advertised TcpServer
    // capability carries the local listener port.
    let node_version = complete_handshake(
        &mut fake,
        network,
        0xfa4e_0002,
        remote_addr.port(), // advertise the dialed port: no upgrade needed
    )
    .await;
    assert_eq!(node_version.network, network);
    assert_eq!(node_version.nonce, local.nonce);
    assert!(
        node_version
            .capabilities
            .iter()
            .any(|c| matches!(c, NodeCapability::TcpServer { port } if *port == local.port()))
    );

    // The dialed endpoint is already the listener endpoint: the
    // `getpeers` view serves it unchanged.
    await_info(&handle, |info| {
        info.connected_peers()
            .iter()
            .any(|p| p.peer_id == peer_id.to_string() && p.address == Some(remote_addr))
    })
    .await;

    // Remote close tears the dialed peer down too.
    drop(fake);
    await_event(
        &mut events,
        |e| matches!(e, NetworkEvent::PeerDisconnected { peer_id: p } if *p == peer_id.to_string()),
    )
    .await;

    handle.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// AllowNewConnection rejections (C# LocalNode.cs:160-174)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn self_connection_is_rejected_by_nonce() {
    let (handle, mut events, port) = start_local_node(ChannelsConfig::default()).await;
    let network = ProtocolSettings::default().network;
    // Learn our own nonce the same way a looped-back connection would.
    let our_nonce = handle.local_node_info().nonce;

    let mut fake = fake_dial(port).await;
    let _node_version = recv_frame(&mut fake).await.expect("node version");
    fake.send(version_message(network, our_nonce, 20333))
        .await
        .expect("send version");

    // No verack: the node drops the connection instead.
    expect_closed(&mut fake).await;
    await_event(&mut events, |e| {
        matches!(e, NetworkEvent::PeerDisconnected { .. })
    })
    .await;
    await_info(&handle, |info| info.connected_peers_count() == 0).await;

    handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn network_magic_mismatch_disconnects() {
    let (handle, mut events, port) = start_local_node(ChannelsConfig::default()).await;
    let wrong_network = ProtocolSettings::default().network.wrapping_add(1);

    let mut fake = fake_dial(port).await;
    let _node_version = recv_frame(&mut fake).await.expect("node version");
    fake.send(version_message(wrong_network, 0xfa4e_0003, 20333))
        .await
        .expect("send version");

    expect_closed(&mut fake).await;
    await_event(&mut events, |e| {
        matches!(e, NetworkEvent::PeerDisconnected { .. })
    })
    .await;

    handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn first_message_must_be_version() {
    let (handle, mut events, port) = start_local_node(ChannelsConfig::default()).await;

    let mut fake = fake_dial(port).await;
    let _node_version = recv_frame(&mut fake).await.expect("node version");
    // C# OnMessage throws ProtocolViolationException for any
    // pre-version command.
    let ping = Message::create(MessageCommand::Ping, Some(&PingPayload::create(0)), false)
        .expect("encode ping");
    fake.send(ping).await.expect("send ping");

    expect_closed(&mut fake).await;
    await_event(&mut events, |e| {
        matches!(e, NetworkEvent::PeerDisconnected { .. })
    })
    .await;

    handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn duplicate_connection_same_address_and_nonce_is_rejected() {
    let (handle, mut events, port) = start_local_node(ChannelsConfig::default()).await;
    let network = ProtocolSettings::default().network;
    let shared_nonce = 0xfa4e_0004;

    // First connection completes the handshake (verack received
    // proves its nonce is recorded).
    let mut first = fake_dial(port).await;
    complete_handshake(&mut first, network, shared_nonce, 20334).await;

    // Second connection from the same IP presenting the same nonce is
    // a duplicate (C# AllowNewConnection's filter) and is dropped.
    let mut second = fake_dial(port).await;
    let _node_version = recv_frame(&mut second).await.expect("node version");
    second
        .send(version_message(network, shared_nonce, 20335))
        .await
        .expect("send version");
    expect_closed(&mut second).await;

    await_event(&mut events, |e| {
        matches!(e, NetworkEvent::PeerDisconnected { .. })
    })
    .await;
    // The first connection survives.
    await_info(&handle, |info| info.connected_peers_count() == 1).await;

    handle.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// Connection caps (C# Peer.cs:272-301)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn max_connections_per_address_rejects_excess_inbound() {
    let config = ChannelsConfig {
        max_connections_per_address: 1,
        ..ChannelsConfig::default()
    };
    let (handle, mut events, port) = start_local_node(config).await;

    // First connection is admitted and greeted with a version.
    let mut first = fake_dial(port).await;
    let NetworkEvent::PeerConnected {
        peer_id: first_id, ..
    } = await_event(&mut events, |e| {
        matches!(e, NetworkEvent::PeerConnected { .. })
    })
    .await
    else {
        unreachable!()
    };
    assert!(
        recv_frame(&mut first).await.is_some(),
        "admitted peer gets a version"
    );

    // Second connection from the same address is aborted before any
    // lifecycle event or version send (C# replies Tcp.Abort without
    // creating the RemoteNode actor).
    let mut second = fake_dial(port).await;
    expect_closed(&mut second).await;
    await_info(&handle, |info| {
        info.connected_peers_count() == 1 && info.connected_peers()[0].peer_id == first_id
    })
    .await;

    handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn max_connections_rejects_excess_inbound() {
    let config = ChannelsConfig {
        max_connections: 1,
        max_connections_per_address: 3,
        ..ChannelsConfig::default()
    };
    let (handle, mut events, port) = start_local_node(config).await;

    let mut first = fake_dial(port).await;
    await_event(&mut events, |e| {
        matches!(e, NetworkEvent::PeerConnected { .. })
    })
    .await;
    assert!(recv_frame(&mut first).await.is_some());

    let mut second = fake_dial(port).await;
    expect_closed(&mut second).await;
    await_info(&handle, |info| info.connected_peers_count() == 1).await;

    handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn dial_respects_connection_caps() {
    let config = ChannelsConfig {
        max_connections: 1,
        ..ChannelsConfig::default()
    };
    let (handle, _events, _port) = start_local_node(config).await;

    let listener_a = TcpListener::bind("127.0.0.1:0").await.expect("bind a");
    let addr_a = listener_a.local_addr().expect("addr a");
    let listener_b = TcpListener::bind("127.0.0.1:0").await.expect("bind b");
    let addr_b = listener_b.local_addr().expect("addr b");
    tokio::spawn(async move {
        let _hold_a = listener_a.accept().await;
        let _hold_b = listener_b.accept().await;
        std::future::pending::<()>().await;
    });

    handle
        .connect_peer(addr_a)
        .await
        .expect("first dial admitted");
    let err = handle
        .connect_peer(addr_b)
        .await
        .expect_err("second dial must hit the connection cap");
    assert!(
        err.to_string().contains("connection limit"),
        "unexpected error: {err}"
    );

    handle.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// Inactivity timeout (C# Connection.cs)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn silent_peer_is_disconnected_after_handshake_timeout() {
    // Drive a RemoteNodeService directly so the test can use a short
    // timeout instead of the C#-faithful 10 s default.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let listen_addr = listener.local_addr().expect("addr");

    let mut fake = {
        let stream = TcpStream::connect(listen_addr).await.expect("dial");
        Framed::new(stream, MessageCodec::new())
    };
    let (stream, remote_addr) = listener.accept().await.expect("accept");

    let identity = Arc::new(LocalIdentity::new(
        ProtocolSettings::default().network,
        7,
        "/neo-rs:test/".to_string(),
        true,
    ));
    let registry = Arc::new(PeerRegistry::with_limits(8, 8));
    let (event_tx, mut events) = broadcast::channel(64);
    let peer_id = PeerId::new();
    let (service, handle) = RemoteNodeService::new(
        stream,
        peer_id,
        remote_addr,
        identity,
        registry.clone(),
        event_tx,
        RemoteNodeState::Handshake,
        CancellationToken::new(),
    );
    assert!(registry.try_admit(peer_id, remote_addr, handle));
    let service = service.with_timeouts(ConnectionTimeouts {
        initial: Duration::from_millis(200),
        idle: Duration::from_millis(200),
    });
    tokio::spawn(service.run());

    // The service greets us, then we stay silent.
    let version = recv_frame(&mut fake).await.expect("node version");
    assert_eq!(version.command, MessageCommand::Version);

    // The timeout must fire, disconnecting the peer and clearing the
    // registry entry.
    await_event(
        &mut events,
        |e| matches!(e, NetworkEvent::PeerDisconnected { peer_id: p } if *p == peer_id.to_string()),
    )
    .await;
    assert!(registry.is_empty());
    expect_closed(&mut fake).await;
}

/// C# `RemoteNode.ProtocolHandler.OnPingMessageReceived`: a post-handshake
/// `ping` is answered with a `pong` carrying the node's own block height and
/// echoing the ping nonce.
#[tokio::test]
async fn ping_is_answered_with_pong_carrying_local_height() {
    let (handle, _events, port) = start_local_node(ChannelsConfig::default()).await;
    let network = ProtocolSettings::default().network;
    let mut fake = fake_dial(port).await;
    complete_handshake(&mut fake, network, 0xfa4e_0002, 20333).await;

    // The fake peer pings advertising height 99; the node must reply with a
    // pong carrying its own (genesis) height 0 and the ping nonce.
    let ping_nonce = 0x4e30_0001;
    let ping_payload = PingPayload::create_with_nonce(99, ping_nonce);
    let ping =
        Message::create(MessageCommand::Ping, Some(&ping_payload), false).expect("encode ping");
    fake.send(ping).await.expect("send ping");

    let pong = loop {
        let frame = recv_frame(&mut fake).await.expect("pong frame");
        if frame.command == MessageCommand::Pong {
            break frame;
        }
    };
    let mut reader = neo_io::MemoryReader::new(&pong.payload_raw);
    let payload =
        <PingPayload as neo_io::Serializable>::deserialize(&mut reader).expect("decode pong");
    assert_eq!(
        payload.last_block_index, 0,
        "pong reports the local genesis height"
    );
    assert_eq!(payload.nonce, ping_nonce, "pong echoes the ping nonce");

    handle.shutdown().await.expect("shutdown");
}

/// C# `RemoteNode.ProtocolHandler.OnInventoryReceived` for a `Block`: a
/// relayed block is decoded and forwarded over the inbound-inventory sink
/// (which the daemon drains into the blockchain service).
#[tokio::test]
async fn relayed_block_is_forwarded_to_the_inventory_sink() {
    let settings = Arc::new(ProtocolSettings::default());
    let (inv_tx, mut inv_rx) = tokio::sync::mpsc::channel::<InboundInventory>(16);
    let (service, handle) = LocalNodeService::with_config(settings, ChannelsConfig::default());
    let service = service.with_inventory_sink(inv_tx);
    tokio::spawn(service.run());
    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");
    let port = handle.local_node_info().port();

    let network = ProtocolSettings::default().network;
    let mut fake = fake_dial(port).await;
    complete_handshake(&mut fake, network, 0xfa4e_0003, 20333).await;

    // Relay a block; the node decodes it and forwards it over the sink.
    let block = neo_payloads::Block::new();
    let block_message =
        Message::create(MessageCommand::Block, Some(&block), false).expect("encode block");
    fake.send(block_message).await.expect("send block");

    let received = tokio::time::timeout(TEST_TIMEOUT, inv_rx.recv())
        .await
        .expect("timed out waiting for relayed inventory")
        .expect("inventory channel open");
    assert!(
        matches!(received, InboundInventory::Block(_)),
        "expected a relayed block on the inventory sink"
    );

    handle.shutdown().await.expect("shutdown");
}

/// C# `TaskManager` block sync: once a peer that is ahead of our ledger
/// completes the handshake, the node requests the next batch of blocks by
/// index (`GetBlockByIndex` starting at `local_height + 1`).
#[tokio::test]
async fn node_requests_blocks_when_peer_is_ahead() {
    let (handle, _events, port) = start_local_node(ChannelsConfig::default()).await;
    let network = ProtocolSettings::default().network;
    let mut fake = fake_dial(port).await;

    // Drive the handshake manually so the peer can advertise a non-zero
    // height via the `FullNode` capability.
    let _node_version = recv_frame(&mut fake).await.expect("node version");
    let capabilities = vec![
        NodeCapability::full_node(100),
        NodeCapability::tcp_server(20333),
    ];
    let payload = VersionPayload::create(
        network,
        0xfa4e_0004,
        "/fake-peer:0.0.1/".to_string(),
        capabilities,
    );
    fake.send(Message::create(MessageCommand::Version, Some(&payload), false).expect("version"))
        .await
        .expect("send version");
    let verack = recv_frame(&mut fake).await.expect("verack");
    assert_eq!(verack.command, MessageCommand::Verack);
    fake.send(verack_message()).await.expect("send verack");

    // The node, at genesis height 0, must request blocks from index 1.
    let request = loop {
        let frame = recv_frame(&mut fake).await.expect("getblockbyindex frame");
        if frame.command == MessageCommand::GetBlockByIndex {
            break frame;
        }
    };
    let mut reader = neo_io::MemoryReader::new(&request.payload_raw);
    let payload = <GetBlockByIndexPayload as neo_io::Serializable>::deserialize(&mut reader)
        .expect("decode getblockbyindex");
    assert_eq!(
        payload.index_start, 1,
        "request starts at local genesis height + 1"
    );
    // C# `TaskManager` requests `Math.Min(endHeight - startHeight, MaxHashesCount)`:
    // the peer advertised height 100, so the node asks for exactly 100 blocks
    // (capped to the peer's height, well under the 500 batch ceiling).
    assert_eq!(payload.count, 100);

    handle.shutdown().await.expect("shutdown");
}

/// Restart/resume cursor: the daemon seeds the network-advertised height from
/// durable `Ledger.CurrentIndex` before accepting peers. A peer that is ahead
/// must see that height in our `version` payload and the first block-sync
/// request must start at `durable_tip + 1`, not at genesis + 1.
#[tokio::test]
async fn seeded_local_height_resumes_block_requests_after_durable_tip() {
    const DURABLE_TIP: u32 = 42;

    let (handle, _events, port) =
        start_local_node_with_seeded_height(ChannelsConfig::default(), DURABLE_TIP).await;
    let network = ProtocolSettings::default().network;
    let mut fake = fake_dial(port).await;

    let node_version = recv_frame(&mut fake).await.expect("node version");
    let node_version = decode_version(&node_version);
    assert!(
        node_version.capabilities.iter().any(|capability| matches!(
            capability,
            NodeCapability::FullNode {
                start_height: DURABLE_TIP
            }
        )),
        "version must advertise the durable ledger tip"
    );

    let capabilities = vec![
        NodeCapability::full_node(100),
        NodeCapability::tcp_server(20333),
    ];
    let payload = VersionPayload::create(
        network,
        0xfa4e_000d,
        "/fake-peer:0.0.1/".to_string(),
        capabilities,
    );
    fake.send(Message::create(MessageCommand::Version, Some(&payload), false).expect("version"))
        .await
        .expect("send version");
    let verack = recv_frame(&mut fake).await.expect("verack");
    assert_eq!(verack.command, MessageCommand::Verack);
    fake.send(verack_message()).await.expect("send verack");

    let request = recv_getblockbyindex(&mut fake).await;
    assert_eq!(
        request.index_start,
        DURABLE_TIP + 1,
        "sync resumes just after the durable tip"
    );
    assert_eq!(request.count, (100 - DURABLE_TIP) as i16);

    handle.shutdown().await.expect("shutdown");
}

/// A stub [`neo_network::BlockSource`] that holds a single block at index 0
/// and answers any block-by-hash lookup with that block.
struct StubBlockSource;
impl neo_network::BlockSource for StubBlockSource {
    fn block_by_index(&self, index: u32) -> Option<neo_payloads::Block> {
        (index == 0).then(neo_payloads::Block::new)
    }
    fn block_by_hash(&self, _hash: &UInt256) -> Option<neo_payloads::Block> {
        Some(neo_payloads::Block::new())
    }
}

struct EmptyBlockSource;
impl neo_network::BlockSource for EmptyBlockSource {
    fn block_by_index(&self, _index: u32) -> Option<neo_payloads::Block> {
        None
    }
}

struct ExtensibleStubSource {
    payload: neo_payloads::ExtensiblePayload,
    hash: UInt256,
}

impl neo_network::BlockSource for ExtensibleStubSource {
    fn block_by_index(&self, _index: u32) -> Option<neo_payloads::Block> {
        None
    }

    fn extensible_by_hash(&self, hash: &UInt256) -> Option<neo_payloads::ExtensiblePayload> {
        (*hash == self.hash).then(|| self.payload.clone())
    }
}

fn sample_extensible_payload() -> neo_payloads::ExtensiblePayload {
    let mut payload = neo_payloads::ExtensiblePayload::new();
    payload.category = "dBFT".to_string();
    payload.valid_block_end = 1;
    payload.data = vec![1, 2, 3];
    payload
}

/// C# `RemoteNode.OnGetBlockByIndexMessageReceived`: a peer's
/// `GetBlockByIndex` is answered by serving the requested blocks from the
/// local ledger as `block` frames.
#[tokio::test]
async fn node_serves_getblockbyindex_from_the_block_source() {
    let settings = Arc::new(ProtocolSettings::default());
    let (service, handle) = LocalNodeService::with_config(settings, ChannelsConfig::default());
    let service = service.with_block_source(Arc::new(StubBlockSource));
    tokio::spawn(service.run());
    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");
    let port = handle.local_node_info().port();

    let network = ProtocolSettings::default().network;
    let mut fake = fake_dial(port).await;
    complete_handshake(&mut fake, network, 0xfa4e_0006, 20333).await;

    // Request block 0; the node serves it as a `block` frame.
    let request = GetBlockByIndexPayload::create(0, 1);
    fake.send(
        Message::create(MessageCommand::GetBlockByIndex, Some(&request), false)
            .expect("encode getblockbyindex"),
    )
    .await
    .expect("send getblockbyindex");

    let block_frame = loop {
        let frame = recv_frame(&mut fake).await.expect("served block frame");
        if frame.command == MessageCommand::Block {
            break frame;
        }
    };
    let mut reader = neo_io::MemoryReader::new(&block_frame.payload_raw);
    <neo_payloads::Block as neo_io::Serializable>::deserialize(&mut reader)
        .expect("served block round-trips");

    handle.shutdown().await.expect("shutdown");
}

/// Starts a local node with the [`StubBlockSource`] and completes a
/// handshake with a fresh fake peer, returning `(handle, fake)`.
async fn local_node_with_block_source(nonce: u32) -> (NetworkHandle, FakeFramed) {
    let settings = Arc::new(ProtocolSettings::default());
    let (service, handle) = LocalNodeService::with_config(settings, ChannelsConfig::default());
    let service = service.with_block_source(Arc::new(StubBlockSource));
    tokio::spawn(service.run());
    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");
    let port = handle.local_node_info().port();
    let network = ProtocolSettings::default().network;
    let mut fake = fake_dial(port).await;
    complete_handshake(&mut fake, network, nonce, 20333).await;
    (handle, fake)
}

async fn local_node_with_empty_source(nonce: u32) -> (NetworkHandle, FakeFramed) {
    let settings = Arc::new(ProtocolSettings::default());
    let (service, handle) = LocalNodeService::with_config(settings, ChannelsConfig::default());
    let service = service.with_block_source(Arc::new(EmptyBlockSource));
    tokio::spawn(service.run());
    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");
    let port = handle.local_node_info().port();
    let network = ProtocolSettings::default().network;
    let mut fake = fake_dial(port).await;
    complete_handshake(&mut fake, network, nonce, 20333).await;
    (handle, fake)
}

/// C# `OnGetHeadersMessageReceived`: a `GetHeaders` request is answered with
/// a `headers` frame carrying the available headers from the start index.
#[tokio::test]
async fn node_serves_getheaders_from_the_block_source() {
    let (handle, mut fake) = local_node_with_block_source(0xfa4e_0007).await;

    let request = GetBlockByIndexPayload::create(0, 10);
    fake.send(
        Message::create(MessageCommand::GetHeaders, Some(&request), false)
            .expect("encode getheaders"),
    )
    .await
    .expect("send getheaders");

    let headers_frame = loop {
        let frame = recv_frame(&mut fake).await.expect("headers frame");
        if frame.command == MessageCommand::Headers {
            break frame;
        }
    };
    let mut reader = neo_io::MemoryReader::new(&headers_frame.payload_raw);
    let payload = <neo_payloads::HeadersPayload as neo_io::Serializable>::deserialize(&mut reader)
        .expect("decode headers");
    // The stub holds a single block (index 0), so one header is served.
    assert_eq!(payload.headers.len(), 1);

    handle.shutdown().await.expect("shutdown");
}

/// C# `OnGetDataMessageReceived`: a `GetData` request for a block hash is
/// answered with the matching `block` frame.
#[tokio::test]
async fn node_serves_getdata_block_from_the_block_source() {
    let (handle, mut fake) = local_node_with_block_source(0xfa4e_0008).await;

    let request = InvPayload::create(InventoryType::Block, &[UInt256::zero()]);
    fake.send(
        Message::create(MessageCommand::GetData, Some(&request), false).expect("encode getdata"),
    )
    .await
    .expect("send getdata");

    let block_frame = loop {
        let frame = recv_frame(&mut fake).await.expect("getdata block frame");
        if frame.command == MessageCommand::Block {
            break frame;
        }
    };
    let mut reader = neo_io::MemoryReader::new(&block_frame.payload_raw);
    <neo_payloads::Block as neo_io::Serializable>::deserialize(&mut reader)
        .expect("served block round-trips");

    handle.shutdown().await.expect("shutdown");
}

/// C# `OnGetDataMessageReceived`: missing block/tx inventory is answered with
/// a grouped `NotFound` payload instead of being silently ignored.
#[tokio::test]
async fn node_replies_notfound_for_missing_getdata_block() {
    let (handle, mut fake) = local_node_with_empty_source(0xfa4e_000e).await;

    let missing_hash = UInt256::zero();
    let request = InvPayload::create(InventoryType::Block, &[missing_hash]);
    fake.send(
        Message::create(MessageCommand::GetData, Some(&request), false).expect("encode getdata"),
    )
    .await
    .expect("send getdata");

    let notfound_frame = loop {
        let frame = recv_frame(&mut fake).await.expect("notfound frame");
        if frame.command == MessageCommand::NotFound {
            break frame;
        }
    };
    let mut reader = neo_io::MemoryReader::new(&notfound_frame.payload_raw);
    let payload =
        <InvPayload as neo_io::Serializable>::deserialize(&mut reader).expect("decode notfound");
    assert_eq!(payload.inventory_type, InventoryType::Block);
    assert_eq!(payload.hashes, vec![missing_hash]);

    handle.shutdown().await.expect("shutdown");
}

/// C# `OnGetDataMessageReceived`: missing transaction inventory is also
/// grouped into a `NotFound` response.
#[tokio::test]
async fn node_replies_notfound_for_missing_getdata_transaction() {
    let (handle, mut fake) = local_node_with_empty_source(0xfa4e_000f).await;

    let missing_hash = UInt256::zero();
    let request = InvPayload::create(InventoryType::Transaction, &[missing_hash]);
    fake.send(
        Message::create(MessageCommand::GetData, Some(&request), false).expect("encode getdata"),
    )
    .await
    .expect("send getdata");

    let notfound_frame = loop {
        let frame = recv_frame(&mut fake).await.expect("notfound frame");
        if frame.command == MessageCommand::NotFound {
            break frame;
        }
    };
    let mut reader = neo_io::MemoryReader::new(&notfound_frame.payload_raw);
    let payload =
        <InvPayload as neo_io::Serializable>::deserialize(&mut reader).expect("decode notfound");
    assert_eq!(payload.inventory_type, InventoryType::Transaction);
    assert_eq!(payload.hashes, vec![missing_hash]);

    handle.shutdown().await.expect("shutdown");
}

/// C# `OnGetDataMessageReceived`: non-block/tx inventory such as
/// `ExtensiblePayload` is served from the relay cache as an `extensible` frame.
#[tokio::test]
async fn node_serves_getdata_extensible_from_the_block_source() {
    let settings = Arc::new(ProtocolSettings::default());
    let (service, handle) = LocalNodeService::with_config(settings, ChannelsConfig::default());
    let mut payload = sample_extensible_payload();
    let hash = payload.hash();
    let service = service.with_block_source(Arc::new(ExtensibleStubSource {
        payload: payload.clone(),
        hash,
    }));
    tokio::spawn(service.run());
    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");
    let port = handle.local_node_info().port();
    let network = ProtocolSettings::default().network;
    let mut fake = fake_dial(port).await;
    complete_handshake(&mut fake, network, 0xfa4e_000c, 20333).await;

    let request = InvPayload::create(InventoryType::Extensible, &[hash]);
    fake.send(
        Message::create(MessageCommand::GetData, Some(&request), false).expect("encode getdata"),
    )
    .await
    .expect("send getdata");

    let extensible_frame = loop {
        let frame = recv_frame(&mut fake)
            .await
            .expect("getdata extensible frame");
        if frame.command == MessageCommand::Extensible {
            break frame;
        }
    };
    let mut reader = neo_io::MemoryReader::new(&extensible_frame.payload_raw);
    let served =
        <neo_payloads::ExtensiblePayload as neo_io::Serializable>::deserialize(&mut reader)
            .expect("served extensible round-trips");
    assert_eq!(served.category, payload.category);
    assert_eq!(served.data, payload.data);

    handle.shutdown().await.expect("shutdown");
}

/// C# `OnInventoryReceived` for an `ExtensiblePayload` (dBFT consensus /
/// state-root messages): a relayed extensible payload is decoded and
/// forwarded over the inbound-inventory sink.
#[tokio::test]
async fn relayed_extensible_is_forwarded_to_the_inventory_sink() {
    let settings = Arc::new(ProtocolSettings::default());
    let (inv_tx, mut inv_rx) = tokio::sync::mpsc::channel::<InboundInventory>(16);
    let (service, handle) = LocalNodeService::with_config(settings, ChannelsConfig::default());
    let service = service.with_inventory_sink(inv_tx);
    tokio::spawn(service.run());
    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");
    let port = handle.local_node_info().port();

    let network = ProtocolSettings::default().network;
    let mut fake = fake_dial(port).await;
    complete_handshake(&mut fake, network, 0xfa4e_0009, 20333).await;

    // A valid payload needs `valid_block_start < valid_block_end`
    // (the decoder rejects an empty range, as it should).
    let mut payload = neo_payloads::ExtensiblePayload::new();
    payload.valid_block_end = 1;
    fake.send(
        Message::create(MessageCommand::Extensible, Some(&payload), false)
            .expect("encode extensible"),
    )
    .await
    .expect("send extensible");

    let received = tokio::time::timeout(TEST_TIMEOUT, inv_rx.recv())
        .await
        .expect("timed out waiting for relayed extensible")
        .expect("inventory channel open");
    assert!(
        matches!(received, InboundInventory::Extensible(_)),
        "expected a relayed extensible payload on the inventory sink"
    );

    handle.shutdown().await.expect("shutdown");
}

/// `broadcast_extensible` relays a dBFT consensus payload to every connected
/// peer as an `Extensible` frame.
#[tokio::test]
async fn broadcast_extensible_reaches_connected_peers() {
    let (handle, _events, port) = start_local_node(ChannelsConfig::default()).await;
    let network = ProtocolSettings::default().network;
    let mut fake = fake_dial(port).await;
    complete_handshake(&mut fake, network, 0xfa4e_000a, 20333).await;
    await_info(&handle, |info| info.connected_peers_count() == 1).await;

    let mut payload = neo_payloads::ExtensiblePayload::new();
    payload.category = "dBFT".to_string();
    payload.valid_block_end = 1; // valid range start(0) < end(1)
    handle
        .broadcast_extensible(payload)
        .await
        .expect("broadcast extensible");

    let frame = loop {
        let f = recv_frame(&mut fake).await.expect("extensible frame");
        if f.command == MessageCommand::Extensible {
            break f;
        }
    };
    let mut reader = neo_io::MemoryReader::new(&frame.payload_raw);
    <neo_payloads::ExtensiblePayload as neo_io::Serializable>::deserialize(&mut reader)
        .expect("extensible round-trips");

    handle.shutdown().await.expect("shutdown");
}

/// C# `RemoteNode.OnInvMessageReceived`: an `Inv` announcing inventory the
/// node does not already hold triggers a `GetData` pull for the unknown hashes.
#[tokio::test]
async fn node_pulls_unknown_inv_with_getdata() {
    let (handle, mut fake) = local_node_with_block_source(0xfa4e_0009).await;

    // The stub holds no transactions, so this hash is unknown and must be pulled.
    let unknown = UInt256::from_bytes(&[0x07u8; 32]).expect("hash");
    let inv = InvPayload::create(InventoryType::Transaction, &[unknown]);
    fake.send(Message::create(MessageCommand::Inv, Some(&inv), false).expect("encode inv"))
        .await
        .expect("send inv");

    let getdata = loop {
        let frame = recv_frame(&mut fake).await.expect("getdata frame");
        if frame.command == MessageCommand::GetData {
            break frame;
        }
    };
    let mut reader = neo_io::MemoryReader::new(&getdata.payload_raw);
    let payload =
        <InvPayload as neo_io::Serializable>::deserialize(&mut reader).expect("decode getdata");
    assert_eq!(payload.inventory_type, InventoryType::Transaction);
    assert_eq!(payload.hashes, vec![unknown]);

    handle.shutdown().await.expect("shutdown");
}

/// Neo N3 v3.10.0 treats extensible payload hashes as fetchable inventory:
/// an `Inv(Extensible)` announcement is pulled with `GetData`, just like
/// blocks and transactions.
#[tokio::test]
async fn node_pulls_unknown_extensible_inv_with_getdata() {
    let (handle, mut fake) = local_node_with_block_source(0xfa4e_000b).await;

    let unknown = UInt256::from_bytes(&[0x2eu8; 32]).expect("hash");
    let inv = InvPayload::create(InventoryType::Extensible, &[unknown]);
    fake.send(Message::create(MessageCommand::Inv, Some(&inv), false).expect("encode inv"))
        .await
        .expect("send inv");

    let getdata = loop {
        let frame = recv_frame(&mut fake).await.expect("getdata frame");
        if frame.command == MessageCommand::GetData {
            break frame;
        }
    };
    let mut reader = neo_io::MemoryReader::new(&getdata.payload_raw);
    let payload =
        <InvPayload as neo_io::Serializable>::deserialize(&mut reader).expect("decode getdata");
    assert_eq!(payload.inventory_type, InventoryType::Extensible);
    assert_eq!(payload.hashes, vec![unknown]);

    handle.shutdown().await.expect("shutdown");
}

/// A [`neo_network::BlockSource`] that reports a single verified mempool tx.
struct MempoolStubSource(UInt256);
impl neo_network::BlockSource for MempoolStubSource {
    fn block_by_index(&self, _index: u32) -> Option<neo_payloads::Block> {
        None
    }
    fn mempool_transaction_hashes(&self) -> Vec<UInt256> {
        vec![self.0]
    }
}

/// C# `RemoteNode.OnMemPoolMessageReceived`: a `Mempool` request is answered
/// with `Inv` announcements of every verified mempool transaction.
#[tokio::test]
async fn node_answers_mempool_request_with_inv() {
    let settings = Arc::new(ProtocolSettings::default());
    let (service, handle) = LocalNodeService::with_config(settings, ChannelsConfig::default());
    let mempool_hash = UInt256::from_bytes(&[0x42u8; 32]).expect("hash");
    let service = service.with_block_source(Arc::new(MempoolStubSource(mempool_hash)));
    tokio::spawn(service.run());
    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");
    let port = handle.local_node_info().port();
    let network = ProtocolSettings::default().network;
    let mut fake = fake_dial(port).await;
    complete_handshake(&mut fake, network, 0xfa4e_000a, 20333).await;

    fake.send(
        Message::from_payload_bytes(MessageCommand::Mempool, Vec::new(), false)
            .expect("encode mempool"),
    )
    .await
    .expect("send mempool");

    let inv = loop {
        let frame = recv_frame(&mut fake).await.expect("inv frame");
        if frame.command == MessageCommand::Inv {
            break frame;
        }
    };
    let mut reader = neo_io::MemoryReader::new(&inv.payload_raw);
    let payload =
        <InvPayload as neo_io::Serializable>::deserialize(&mut reader).expect("decode inv");
    assert_eq!(payload.inventory_type, InventoryType::Transaction);
    assert_eq!(payload.hashes, vec![mempool_hash]);

    handle.shutdown().await.expect("shutdown");
}

/// C# `LocalNode.RelayDirectly`: `broadcast_inv` announces inventory hashes to
/// every connected peer via an `Inv` message.
#[tokio::test]
async fn broadcast_inv_reaches_connected_peers() {
    let (handle, _events, port) = start_local_node(ChannelsConfig::default()).await;
    let network = ProtocolSettings::default().network;
    let mut fake = fake_dial(port).await;
    complete_handshake(&mut fake, network, 0xfa4e_000b, 20333).await;
    await_info(&handle, |info| info.connected_peers_count() == 1).await;

    let announced = UInt256::from_bytes(&[0x55u8; 32]).expect("hash");
    handle
        .broadcast_inv(InventoryType::Transaction, vec![announced])
        .await
        .expect("broadcast inv");

    let frame = loop {
        let f = recv_frame(&mut fake).await.expect("inv frame");
        if f.command == MessageCommand::Inv {
            break f;
        }
    };
    let mut reader = neo_io::MemoryReader::new(&frame.payload_raw);
    let payload =
        <InvPayload as neo_io::Serializable>::deserialize(&mut reader).expect("decode inv");
    assert_eq!(payload.inventory_type, InventoryType::Transaction);
    assert_eq!(payload.hashes, vec![announced]);

    handle.shutdown().await.expect("shutdown");
}

/// Helper: receive the next `GetBlockByIndex` request and decode its payload.
async fn recv_getblockbyindex(fake: &mut FakeFramed) -> GetBlockByIndexPayload {
    loop {
        let frame = recv_frame(fake).await.expect("getblockbyindex frame");
        if frame.command == MessageCommand::GetBlockByIndex {
            let mut reader = neo_io::MemoryReader::new(&frame.payload_raw);
            return <GetBlockByIndexPayload as neo_io::Serializable>::deserialize(&mut reader)
                .expect("decode getblockbyindex");
        }
    }
}

/// C# `TaskManager` pipelining: while a peer is far ahead, the node requests a
/// 500-block window, then advances the request cursor forward as the ledger
/// persists — it does NOT re-request from the genesis tip each tick.
#[tokio::test]
async fn node_pipelines_block_requests_as_height_advances() {
    let (handle, _events, port) = start_local_node(ChannelsConfig::default()).await;
    let network = ProtocolSettings::default().network;
    let mut fake = fake_dial(port).await;

    // Drive the handshake so the peer advertises a height far ahead (5000).
    let _node_version = recv_frame(&mut fake).await.expect("node version");
    let capabilities = vec![
        NodeCapability::full_node(5000),
        NodeCapability::tcp_server(20333),
    ];
    let payload = VersionPayload::create(
        network,
        0xfa4e_000c,
        "/fake-peer:0.0.1/".to_string(),
        capabilities,
    );
    fake.send(Message::create(MessageCommand::Version, Some(&payload), false).expect("version"))
        .await
        .expect("send version");
    let verack = recv_frame(&mut fake).await.expect("verack");
    assert_eq!(verack.command, MessageCommand::Verack);
    fake.send(verack_message()).await.expect("send verack");

    // First request: a full 500-block window from genesis + 1 (peer far ahead,
    // so the batch is capped by MaxHashesCount, not the peer height).
    let first = recv_getblockbyindex(&mut fake).await;
    assert_eq!(first.index_start, 1);
    assert_eq!(first.count, 500);

    // Simulate the ledger persisting that window; the node must pipeline the
    // next window forward (501..) rather than re-requesting from index 1.
    handle.set_block_height(500).await.expect("advance height");
    let second = recv_getblockbyindex(&mut fake).await;
    assert_eq!(
        second.index_start, 501,
        "request pipelines forward past the persisted tip"
    );

    handle.shutdown().await.expect("shutdown");
}
