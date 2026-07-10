use super::*;

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
    payload.sender = payload.witness.script_hash();
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
pub(super) async fn local_node_with_block_source(nonce: u32) -> (NetworkHandle, FakeFramed) {
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
