use super::*;

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
