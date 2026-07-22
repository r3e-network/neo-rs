//! Connection lifecycle and unsolicited-traffic regressions for sync peers.

use super::*;

#[tokio::test]
async fn silent_peer_is_disconnected_after_handshake_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let listen_addr = listener.local_addr().expect("addr");

    let mut fake = {
        let stream = TcpStream::connect(listen_addr).await.expect("dial");
        Framed::new(stream, MessageCodec::new())
    };
    let (stream, remote_addr) = listener.accept().await.expect("accept");

    let identity = Arc::new(LocalIdentity::new(
        network_magic(),
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

    let version = recv_frame(&mut fake).await.expect("node version");
    assert_eq!(version.command, MessageCommand::Version);
    await_event(
        &mut events,
        |event| {
            matches!(event, NetworkEvent::PeerDisconnected { peer_id: id } if *id == peer_id.to_string())
        },
    )
    .await;
    assert!(registry.is_empty());
    expect_closed(&mut fake).await;
}

/// C# `RemoteNode.ProtocolHandler.OnPingMessageReceived`: echo the nonce and
/// report the local block height.
#[tokio::test]
async fn ping_is_answered_with_pong_carrying_local_height() {
    let (handle, _events, port) = start_local_node(ChannelsConfig::default()).await;
    let network = network_magic();
    let mut fake = fake_dial(port).await;
    complete_handshake(&mut fake, network, 0xfa4e_0002, 20333).await;

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
    assert_eq!(payload.last_block_index, 0);
    assert_eq!(payload.nonce, ping_nonce);

    handle.shutdown().await.expect("shutdown");
}

/// Relayed blocks continue through the generic inventory path when no
/// coordinator range owns them.
#[tokio::test]
async fn relayed_block_is_forwarded_to_the_inventory_sink() {
    let (inv_tx, mut inv_rx) = tokio::sync::mpsc::channel::<InboundInventory>(16);
    let (service, handle) =
        LocalNodeService::with_config(test_chain_spec(), ChannelsConfig::default());
    let service = service.with_inventory_sink(inv_tx);
    tokio::spawn(service.run());
    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");

    let mut fake = fake_dial(handle.local_node_info().port()).await;
    complete_handshake(&mut fake, network_magic(), 0xfa4e_0003, 20333).await;
    let block = neo_payloads::Block::new();
    fake.send(Message::create(MessageCommand::Block, Some(&block), false).expect("block"))
        .await
        .expect("send block");

    let received = tokio::time::timeout(TEST_TIMEOUT, inv_rx.recv())
        .await
        .expect("timed out waiting for relayed inventory")
        .expect("inventory channel open");
    assert!(matches!(received, InboundInventory::Block(_)));
    handle.shutdown().await.expect("shutdown");
}

/// Ready peers never initiate range sync; the shared coordinator owns every
/// `GetHeaders` and `GetBlockByIndex` assignment.
#[tokio::test]
async fn ready_peer_does_not_issue_unsolicited_block_requests() {
    let (handle, _events, port) = start_local_node(ChannelsConfig::default()).await;
    let network = network_magic();
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
    assert_eq!(
        recv_frame(&mut fake).await.expect("verack").command,
        MessageCommand::Verack
    );
    fake.send(verack_message()).await.expect("send verack");

    match tokio::time::timeout(Duration::from_millis(350), fake.next()).await {
        Err(_) => {}
        Ok(Some(Ok(message))) => panic!("unexpected {:?} frame", message.command),
        Ok(Some(Err(error))) => panic!("frame decode failed: {error}"),
        Ok(None) => panic!("peer connection closed"),
    }
    handle.shutdown().await.expect("shutdown");
}
