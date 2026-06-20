use super::*;

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
