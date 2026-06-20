use super::*;

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
