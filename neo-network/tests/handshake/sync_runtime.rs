use super::*;

fn block_message(index: u32) -> Message {
    let mut header = neo_payloads::Header::new();
    header.set_index(index);
    let block = neo_payloads::Block::from_parts(header, vec![]);
    Message::create(MessageCommand::Block, Some(&block), false).expect("encode block")
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
        block_fetch: Duration::from_secs(1),
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

/// Ready peers do not issue unsolicited range requests. The shared downloader
/// coordinator is the sole owner of `GetBlockByIndex` assignments.
#[tokio::test]
async fn ready_peer_does_not_issue_unsolicited_block_requests() {
    let (handle, _events, port) = start_local_node(ChannelsConfig::default()).await;
    let network = ProtocolSettings::default().network;
    let mut fake = fake_dial(port).await;
    let _node_version = recv_frame(&mut fake).await.expect("node version");
    let payload = VersionPayload::create(
        network,
        0xfa4e_0010,
        "/fake-peer:0.0.1/".to_string(),
        0,
        vec![
            NodeCapability::full_node(100),
            NodeCapability::tcp_server(20333),
        ],
    );
    fake.send(Message::create(MessageCommand::Version, Some(&payload), false).expect("version"))
        .await
        .expect("send version");
    let verack = recv_frame(&mut fake).await.expect("verack");
    assert_eq!(verack.command, MessageCommand::Verack);
    fake.send(verack_message()).await.expect("send verack");

    match tokio::time::timeout(Duration::from_millis(350), fake.next()).await {
        Err(_) => {}
        Ok(Some(Ok(message))) => panic!(
            "peer session should not emit unsolicited {:?} frame",
            message.command
        ),
        Ok(Some(Err(err))) => panic!("frame decode failed while checking sync ownership: {err}"),
        Ok(None) => panic!("peer connection closed while checking sync ownership"),
    }

    handle.shutdown().await.expect("shutdown");
}

/// Explicit range fetches collect matching `block` frames into a downloader
/// batch while leaving unrelated relayed blocks on the normal inventory path.
#[tokio::test]
async fn remote_node_handle_fetches_explicit_block_range_as_batch() {
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
    let (event_tx, _events) = broadcast::channel(64);
    let (inv_tx, mut inv_rx) = tokio::sync::mpsc::channel::<InboundInventory>(8);
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
    let service = service.with_inventory_sink(inv_tx);
    assert!(registry.try_admit(peer_id, remote_addr, handle.clone()));
    tokio::spawn(service.run());

    complete_handshake(
        &mut fake,
        ProtocolSettings::default().network,
        0xfa4e_0012,
        20333,
    )
    .await;
    await_download_peer_ready(&registry, peer_id).await;

    let fetch = tokio::spawn({
        let handle = handle.clone();
        async move { handle.fetch_blocks_by_index(BlockRequest::new(7, 3)).await }
    });
    let request = recv_getblockbyindex(&mut fake).await;
    assert_eq!(request.index_start, 7);
    assert_eq!(request.count, 3);

    fake.send(block_message(99))
        .await
        .expect("send unrelated block");
    let relayed = tokio::time::timeout(TEST_TIMEOUT, inv_rx.recv())
        .await
        .expect("timed out waiting for unrelated inventory")
        .expect("inventory channel open");
    let InboundInventory::Block(relayed) = relayed else {
        panic!("expected unrelated block inventory");
    };
    assert_eq!(relayed.index(), 99);

    for index in 7..=9 {
        fake.send(block_message(index))
            .await
            .expect("send requested block");
    }

    let batch = fetch
        .await
        .expect("fetch task joined")
        .expect("fetch succeeded");
    assert_eq!(batch.peer_id, Some(peer_id));
    assert_eq!(batch.start_height, 7);
    assert_eq!(
        batch
            .blocks
            .iter()
            .map(neo_payloads::Block::index)
            .collect::<Vec<_>>(),
        vec![7, 8, 9]
    );

    handle.shutdown().await.expect("shutdown");
}

/// A peer cannot keep an incomplete coordinator assignment alive by sending
/// unrelated traffic. Fetch expiry clears the per-peer correlation state while
/// leaving the healthy connection available for a later assignment.
#[tokio::test]
async fn block_fetch_timeout_is_absolute_and_clears_pending_assignment() {
    const FETCH_TIMEOUT: Duration = Duration::from_millis(150);

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
    let (event_tx, _events) = broadcast::channel(64);
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
    let service = service.with_timeouts(ConnectionTimeouts {
        initial: Duration::from_secs(2),
        idle: Duration::from_secs(2),
        block_fetch: FETCH_TIMEOUT,
    });
    assert!(registry.try_admit(peer_id, remote_addr, handle.clone()));
    tokio::spawn(service.run());

    complete_handshake(
        &mut fake,
        ProtocolSettings::default().network,
        0xfa4e_0014,
        20333,
    )
    .await;
    await_download_peer_ready(&registry, peer_id).await;

    let first_fetch = tokio::spawn({
        let handle = handle.clone();
        async move { handle.fetch_blocks_by_index(BlockRequest::new(7, 3)).await }
    });
    let first_request = recv_getblockbyindex(&mut fake).await;
    assert_eq!(first_request.index_start, 7);
    assert_eq!(first_request.count, 3);
    fake.send(block_message(7))
        .await
        .expect("send first requested block");

    let ping_nonce = 0x4e30_0014;
    let ping_payload = PingPayload::create_with_nonce(99, ping_nonce);
    fake.send(Message::create(MessageCommand::Ping, Some(&ping_payload), false).expect("ping"))
        .await
        .expect("send ping while fetch is pending");
    let pong = recv_frame(&mut fake)
        .await
        .expect("pong while fetch is pending");
    assert_eq!(pong.command, MessageCommand::Pong);

    let first_error = tokio::time::timeout(Duration::from_secs(1), first_fetch)
        .await
        .expect("block fetch did not expire")
        .expect("fetch task joined")
        .expect_err("incomplete block fetch must time out");
    match first_error {
        neo_network::NetworkError::RemoteUnavailable {
            peer_id: failed_peer,
            detail,
        } => {
            assert_eq!(failed_peer, peer_id.to_string());
            assert!(detail.contains("block range [7, 10)"), "{detail}");
            assert!(detail.contains("150ms"), "{detail}");
        }
        other => panic!("expected remote-unavailable timeout, got {other}"),
    }

    let second_fetch = tokio::spawn({
        let handle = handle.clone();
        async move { handle.fetch_blocks_by_index(BlockRequest::new(20, 1)).await }
    });
    let second_request = recv_getblockbyindex(&mut fake).await;
    assert_eq!(second_request.index_start, 20);
    assert_eq!(second_request.count, 1);
    fake.send(block_message(20))
        .await
        .expect("complete second block fetch");

    let second_batch = second_fetch
        .await
        .expect("second fetch task joined")
        .expect("second fetch succeeds after timeout cleanup");
    assert_eq!(second_batch.start_height, 20);
    assert_eq!(second_batch.blocks.len(), 1);
    assert_eq!(second_batch.blocks[0].index(), 20);

    handle.shutdown().await.expect("shutdown");
}

/// Coordinator range requests are a ready-session operation, not generic
/// outbound messages. Rejecting them before `verack` prevents an expired
/// request from remaining in the handshake queue and flushing later.
#[tokio::test]
async fn block_fetch_before_verack_is_rejected_without_queuing_stale_request() {
    const FETCH_TIMEOUT: Duration = Duration::from_millis(150);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let listen_addr = listener.local_addr().expect("addr");

    let mut fake = {
        let stream = TcpStream::connect(listen_addr).await.expect("dial");
        Framed::new(stream, MessageCodec::new())
    };
    let (stream, remote_addr) = listener.accept().await.expect("accept");

    let network = ProtocolSettings::default().network;
    let identity = Arc::new(LocalIdentity::new(
        network,
        7,
        "/neo-rs:test/".to_string(),
        true,
    ));
    let registry = Arc::new(PeerRegistry::with_limits(8, 8));
    let (event_tx, _events) = broadcast::channel(64);
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
    let service = service.with_timeouts(ConnectionTimeouts {
        initial: Duration::from_secs(2),
        idle: Duration::from_secs(2),
        block_fetch: FETCH_TIMEOUT,
    });
    assert!(registry.try_admit(peer_id, remote_addr, handle.clone()));
    tokio::spawn(service.run());

    let node_version = recv_frame(&mut fake).await.expect("node version");
    assert_eq!(node_version.command, MessageCommand::Version);
    fake.send(version_message(network, 0xfa4e_0015, 20333))
        .await
        .expect("send peer version");
    let node_verack = recv_frame(&mut fake).await.expect("node verack");
    assert_eq!(node_verack.command, MessageCommand::Verack);

    let pre_ready_error = handle
        .fetch_blocks_by_index(BlockRequest::new(7, 3))
        .await
        .expect_err("fetch before peer verack must be rejected");
    match pre_ready_error {
        neo_network::NetworkError::RemoteUnavailable {
            peer_id: failed_peer,
            detail,
        } => {
            assert_eq!(failed_peer, peer_id.to_string());
            assert!(detail.contains("handshake"), "{detail}");
        }
        other => panic!("expected pre-handshake remote-unavailable error, got {other}"),
    }

    tokio::time::sleep(FETCH_TIMEOUT + Duration::from_millis(50)).await;
    fake.send(verack_message()).await.expect("send peer verack");
    match tokio::time::timeout(Duration::from_millis(100), fake.next()).await {
        Err(_) => {}
        Ok(Some(Ok(message))) => panic!(
            "pre-handshake fetch must not flush a stale {:?} frame",
            message.command
        ),
        Ok(Some(Err(err))) => panic!("frame decode failed after verack: {err}"),
        Ok(None) => panic!("peer connection closed after verack"),
    }
    await_download_peer_ready(&registry, peer_id).await;

    let ready_fetch = tokio::spawn({
        let handle = handle.clone();
        async move { handle.fetch_blocks_by_index(BlockRequest::new(20, 1)).await }
    });
    let ready_request = recv_getblockbyindex(&mut fake).await;
    assert_eq!(ready_request.index_start, 20);
    assert_eq!(ready_request.count, 1);
    fake.send(block_message(20))
        .await
        .expect("complete ready fetch");
    let batch = ready_fetch
        .await
        .expect("ready fetch task joined")
        .expect("ready fetch succeeds");
    assert_eq!(batch.start_height, 20);
    assert_eq!(batch.blocks.len(), 1);

    handle.shutdown().await.expect("shutdown");
}

/// The connected-peer registry is the transport fetcher used by
/// `BlockDownloadCoordinator`: it resolves the assigned peer handle and returns
/// the collected batch.
#[tokio::test]
async fn peer_registry_fetcher_collects_assigned_peer_range() {
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
    let (event_tx, _events) = broadcast::channel(64);
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
    assert!(registry.try_admit(peer_id, remote_addr, handle.clone()));
    tokio::spawn(service.run());

    complete_handshake(
        &mut fake,
        ProtocolSettings::default().network,
        0xfa4e_0013,
        20333,
    )
    .await;
    await_download_peer_ready(&registry, peer_id).await;

    let fetch = tokio::spawn({
        let registry = registry.clone();
        async move {
            neo_network::BlockRangeFetcher::fetch_range(
                &registry,
                BlockRangeAssignment::new(peer_id, BlockRequest::new(11, 2), 0),
            )
            .await
        }
    });
    let request = recv_getblockbyindex(&mut fake).await;
    assert_eq!(request.index_start, 11);
    assert_eq!(request.count, 2);

    for index in 11..=12 {
        fake.send(block_message(index))
            .await
            .expect("send requested block");
    }

    let batch = fetch
        .await
        .expect("fetch task joined")
        .expect("fetch succeeded");
    assert_eq!(batch.peer_id, Some(peer_id));
    assert_eq!(batch.start_height, 11);
    assert_eq!(
        batch
            .blocks
            .iter()
            .map(neo_payloads::Block::index)
            .collect::<Vec<_>>(),
        vec![11, 12]
    );

    handle.shutdown().await.expect("shutdown");
}

/// The daemon seeds the network-advertised height from durable
/// `Ledger.CurrentIndex` before accepting peers. Range assignment remains owned
/// by the coordinator, so advertising a durable tip must not trigger a request
/// from the peer session itself.
#[tokio::test]
async fn seeded_local_height_is_advertised_without_unsolicited_sync() {
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
        0,
        capabilities,
    );
    fake.send(Message::create(MessageCommand::Version, Some(&payload), false).expect("version"))
        .await
        .expect("send version");
    let verack = recv_frame(&mut fake).await.expect("verack");
    assert_eq!(verack.command, MessageCommand::Verack);
    fake.send(verack_message()).await.expect("send verack");

    match tokio::time::timeout(Duration::from_millis(350), fake.next()).await {
        Err(_) => {}
        Ok(Some(Ok(message))) => panic!(
            "peer session should not emit unsolicited {:?} frame",
            message.command
        ),
        Ok(Some(Err(err))) => panic!("frame decode failed while checking sync ownership: {err}"),
        Ok(None) => panic!("peer connection closed while checking sync ownership"),
    }

    handle.shutdown().await.expect("shutdown");
}
