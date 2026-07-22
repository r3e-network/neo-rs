//! Successful and malformed correlated block/header range regressions.

use super::*;

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
        network_magic(),
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

    complete_handshake(&mut fake, network_magic(), 0xfa4e_0012, 20333).await;
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
    assert_eq!(
        batch
            .blocks
            .iter()
            .map(neo_payloads::Block::index)
            .collect::<Vec<_>>(),
        vec![7, 8, 9]
    );

    let malformed = tokio::spawn({
        let handle = handle.clone();
        async move { handle.fetch_blocks_by_index(BlockRequest::new(20, 3)).await }
    });
    assert_eq!(recv_getblockbyindex(&mut fake).await.index_start, 20);
    fake.send(block_message(21))
        .await
        .expect("send out-of-order in-range block");
    let error = tokio::time::timeout(Duration::from_secs(1), malformed)
        .await
        .expect("malformed response must fail immediately")
        .expect("fetch task joined")
        .expect_err("out-of-order response must fail");
    assert!(error.to_string().contains("expected block 20"), "{error}");
    assert!(
        tokio::time::timeout(Duration::from_millis(100), inv_rx.recv())
            .await
            .is_err()
    );

    let retry = tokio::spawn({
        let handle = handle.clone();
        async move { handle.fetch_blocks_by_index(BlockRequest::new(30, 1)).await }
    });
    assert_eq!(recv_getblockbyindex(&mut fake).await.index_start, 30);
    fake.send(block_message(30)).await.expect("complete retry");
    assert_eq!(
        retry
            .await
            .expect("retry joined")
            .expect("retry succeeds")
            .blocks[0]
            .index(),
        30
    );

    let trailing_fetch = tokio::spawn({
        let handle = handle.clone();
        async move { handle.fetch_blocks_by_index(BlockRequest::new(40, 1)).await }
    });
    assert_eq!(recv_getblockbyindex(&mut fake).await.index_start, 40);
    let mut trailing = block_message(40);
    trailing.payload_raw.push(0xff);
    trailing.payload_compressed = trailing.payload_raw.clone();
    fake.send(trailing)
        .await
        .expect("send trailing block bytes");
    let error = trailing_fetch
        .await
        .expect("trailing fetch joined")
        .expect_err("trailing bytes must fail");
    assert!(error.to_string().contains("trailing"), "{error}");
    handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn remote_node_handle_fetches_explicit_header_range_as_batch() {
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

    complete_handshake(&mut fake, network_magic(), 0xfa4e_0013, 20333).await;
    await_download_peer_ready(&registry, peer_id).await;
    let fetch = tokio::spawn({
        let handle = handle.clone();
        async move {
            handle
                .fetch_headers_by_index(neo_network::HeaderRequest::new(7, 5))
                .await
        }
    });
    let request = recv_getheaders(&mut fake).await;
    assert_eq!(request.index_start, 7);
    assert_eq!(request.count, 5);
    fake.send(block_message(99))
        .await
        .expect("send unrelated block");
    let relayed = tokio::time::timeout(TEST_TIMEOUT, inv_rx.recv())
        .await
        .expect("timed out waiting for relay")
        .expect("inventory channel open");
    let InboundInventory::Block(relayed) = relayed else {
        panic!("expected unrelated block inventory");
    };
    assert_eq!(relayed.index(), 99);

    fake.send(headers_message(&[7, 8, 9]))
        .await
        .expect("send short header batch");
    let batch = fetch.await.expect("fetch joined").expect("fetch succeeds");
    assert_eq!(batch.peer_id, Some(peer_id));
    assert_eq!(batch.start_height, 7);
    assert_eq!(
        batch
            .headers
            .iter()
            .map(neo_payloads::Header::index)
            .collect::<Vec<_>>(),
        vec![7, 8, 9]
    );
    handle.shutdown().await.expect("shutdown");
}
