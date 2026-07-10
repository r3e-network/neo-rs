use super::block_source::local_node_with_block_source;
use super::*;

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
    payload.sender = payload.witness.script_hash();
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
    payload.sender = payload.witness.script_hash();
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

/// Neo N3 v3.10.1 still treats extensible payload hashes as fetchable
/// inventory: an `Inv(Extensible)` announcement is pulled with `GetData`, just
/// like blocks and transactions.
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
