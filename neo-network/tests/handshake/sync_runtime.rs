use super::*;

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

/// C# `TaskManager` pipelining: while a peer is far ahead, the node requests a
/// 500-block window, then advances the request cursor forward as the ledger
/// persists; it does NOT re-request from the genesis tip each tick.
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

/// Sync throughput: a far-ahead peer should keep the per-peer request pipeline
/// two protocol-sized windows ahead of the durable tip. Each
/// `GetBlockByIndex` still respects Neo's 500-block wire limit, but the node
/// should not leave the transport idle while the first window is being
/// persisted.
#[tokio::test]
async fn node_keeps_two_block_request_windows_in_flight() {
    let (handle, _events, port) = start_local_node(ChannelsConfig::default()).await;
    let network = ProtocolSettings::default().network;
    let mut fake = fake_dial(port).await;

    let _node_version = recv_frame(&mut fake).await.expect("node version");
    let capabilities = vec![
        NodeCapability::full_node(5000),
        NodeCapability::tcp_server(20333),
    ];
    let payload = VersionPayload::create(
        network,
        0xfa4e_000e,
        "/fake-peer:0.0.1/".to_string(),
        capabilities,
    );
    fake.send(Message::create(MessageCommand::Version, Some(&payload), false).expect("version"))
        .await
        .expect("send version");
    let verack = recv_frame(&mut fake).await.expect("verack");
    assert_eq!(verack.command, MessageCommand::Verack);
    fake.send(verack_message()).await.expect("send verack");

    let first = recv_getblockbyindex(&mut fake).await;
    assert_eq!(first.index_start, 1);
    assert_eq!(first.count, 500);

    let second = recv_getblockbyindex(&mut fake).await;
    assert_eq!(
        second.index_start, 501,
        "sync should keep a second request window in flight"
    );
    assert_eq!(
        second.count, 500,
        "each request must still obey the protocol request cap"
    );

    handle.shutdown().await.expect("shutdown");
}

/// Sync cadence: the second in-flight window must be requested quickly enough
/// to support a 1000 BPS target on a single responsive peer. The request still
/// waits for the periodic sync tick, but that tick should be sub-250ms rather
/// than the old half-second cadence.
#[tokio::test]
async fn node_requests_second_sync_window_without_half_second_idle() {
    let (handle, _events, port) = start_local_node(ChannelsConfig::default()).await;
    let network = ProtocolSettings::default().network;
    let mut fake = fake_dial(port).await;

    let _node_version = recv_frame(&mut fake).await.expect("node version");
    let capabilities = vec![
        NodeCapability::full_node(5000),
        NodeCapability::tcp_server(20333),
    ];
    let payload = VersionPayload::create(
        network,
        0xfa4e_000f,
        "/fake-peer:0.0.1/".to_string(),
        capabilities,
    );
    fake.send(Message::create(MessageCommand::Version, Some(&payload), false).expect("version"))
        .await
        .expect("send version");
    let verack = recv_frame(&mut fake).await.expect("verack");
    assert_eq!(verack.command, MessageCommand::Verack);
    fake.send(verack_message()).await.expect("send verack");

    let first = recv_getblockbyindex(&mut fake).await;
    assert_eq!(first.index_start, 1);
    assert_eq!(first.count, 500);

    let second = tokio::time::timeout(Duration::from_millis(250), recv_getblockbyindex(&mut fake))
        .await
        .expect("second sync request should not wait for a half-second timer");
    assert_eq!(second.index_start, 501);
    assert_eq!(second.count, 500);

    handle.shutdown().await.expect("shutdown");
}

/// Recovery should not mistake a full pipeline for a dropped response. With a
/// 100ms sync cadence, a three-tick stall threshold would rewind in about
/// 300ms and duplicate the first request window before a real node has had a
/// fair chance to persist the in-flight blocks.
#[tokio::test]
async fn node_does_not_rewind_sync_cursor_before_stall_timeout() {
    let (handle, _events, port) = start_local_node(ChannelsConfig::default()).await;
    let network = ProtocolSettings::default().network;
    let mut fake = fake_dial(port).await;

    let _node_version = recv_frame(&mut fake).await.expect("node version");
    let capabilities = vec![
        NodeCapability::full_node(5000),
        NodeCapability::tcp_server(20333),
    ];
    let payload = VersionPayload::create(
        network,
        0xfa4e_0010,
        "/fake-peer:0.0.1/".to_string(),
        capabilities,
    );
    fake.send(Message::create(MessageCommand::Version, Some(&payload), false).expect("version"))
        .await
        .expect("send version");
    let verack = recv_frame(&mut fake).await.expect("verack");
    assert_eq!(verack.command, MessageCommand::Verack);
    fake.send(verack_message()).await.expect("send verack");

    let first = recv_getblockbyindex(&mut fake).await;
    assert_eq!(first.index_start, 1);
    assert_eq!(first.count, 500);
    let second = recv_getblockbyindex(&mut fake).await;
    assert_eq!(second.index_start, 501);
    assert_eq!(second.count, 500);

    let duplicate =
        tokio::time::timeout(Duration::from_millis(500), recv_getblockbyindex(&mut fake)).await;
    assert!(
        duplicate.is_err(),
        "sync should not duplicate an in-flight request window before the stall timeout"
    );

    handle.shutdown().await.expect("shutdown");
}
