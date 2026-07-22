use super::*;

use neo_crypto::BloomFilter;
use neo_payloads::{Signer, Transaction, Witness, WitnessScope};
use neo_primitives::UInt160;

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

struct FilteredBlockSource {
    block: neo_payloads::Block,
}

impl neo_network::BlockSource for FilteredBlockSource {
    fn block_by_index(&self, index: u32) -> Option<neo_payloads::Block> {
        (index == 0).then(|| self.block.clone())
    }

    fn block_by_hash(&self, _hash: &UInt256) -> Option<neo_payloads::Block> {
        Some(self.block.clone())
    }
}

fn filtered_transaction(account: UInt160) -> Transaction {
    let mut transaction = Transaction::new();
    transaction.set_version(0);
    transaction.set_nonce(1);
    transaction.set_system_fee(0);
    transaction.set_network_fee(0);
    transaction.set_valid_until_block(100);
    transaction.set_signers(vec![Signer::new(account, WitnessScope::NONE)]);
    transaction.set_attributes(Vec::new());
    transaction.set_script(vec![0x01]);
    transaction.set_witnesses(vec![Witness::empty()]);
    transaction
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
    let (service, handle) =
        LocalNodeService::with_config(test_chain_spec(), ChannelsConfig::default());
    let service = service.with_block_source(Arc::new(StubBlockSource));
    tokio::spawn(service.run());
    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");
    let port = handle.local_node_info().port();

    let network = network_magic();
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
    let (service, handle) =
        LocalNodeService::with_config(test_chain_spec(), ChannelsConfig::default());
    let service = service.with_block_source(Arc::new(StubBlockSource));
    tokio::spawn(service.run());
    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");
    let port = handle.local_node_info().port();
    let network = network_magic();
    let mut fake = fake_dial(port).await;
    complete_handshake(&mut fake, network, nonce, 20333).await;
    (handle, fake)
}

async fn local_node_with_empty_source(nonce: u32) -> (NetworkHandle, FakeFramed) {
    let (service, handle) =
        LocalNodeService::with_config(test_chain_spec(), ChannelsConfig::default());
    let service = service.with_block_source(Arc::new(EmptyBlockSource));
    tokio::spawn(service.run());
    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");
    let port = handle.local_node_info().port();
    let network = network_magic();
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

/// C# `OnFilterLoadMessageReceived`, `OnFilterAddMessageReceived`, and
/// `OnGetDataMessageReceived`: a matching SPV peer receives a merkle block,
/// and `FilterClear` restores full-block responses.
#[tokio::test]
async fn node_serves_filtered_blocks_and_clears_filter() {
    let account = UInt160::from_array([0x11; UInt160::LENGTH]);
    let transaction = filtered_transaction(account);
    let mut block =
        neo_payloads::Block::from_parts(neo_payloads::Header::new(), vec![transaction.clone()]);
    block.rebuild_merkle_root();
    let (service, handle) =
        LocalNodeService::with_config(test_chain_spec(), ChannelsConfig::default());
    let service = service.with_block_source(Arc::new(FilteredBlockSource { block }));
    tokio::spawn(service.run());
    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");
    let mut fake = fake_dial(handle.local_node_info().port()).await;
    complete_handshake(&mut fake, network_magic(), 0xfa4e_0010, 20333).await;

    // Start with an empty filter, then exercise FilterAdd against the signer
    // account. This covers Neo's transaction-hash OR signer-account match.
    let empty_filter = BloomFilter::new(256, 3, 0).expect("empty bloom filter");
    let filter_load =
        neo_payloads::p2p_payloads::FilterLoadPayload::create_from_bloom_filter(&empty_filter);
    fake.send(
        Message::create(MessageCommand::FilterLoad, Some(&filter_load), false)
            .expect("encode filterload"),
    )
    .await
    .expect("send filterload");
    let filter_add = neo_payloads::p2p_payloads::FilterAddPayload::new(account.to_bytes());
    fake.send(
        Message::create(MessageCommand::FilterAdd, Some(&filter_add), false)
            .expect("encode filteradd"),
    )
    .await
    .expect("send filteradd");

    let request = InvPayload::create(InventoryType::Block, &[UInt256::zero()]);
    fake.send(
        Message::create(MessageCommand::GetData, Some(&request), false).expect("encode getdata"),
    )
    .await
    .expect("send getdata");
    let merkle_frame = loop {
        let frame = recv_frame(&mut fake).await.expect("filtered block frame");
        if frame.command == MessageCommand::MerkleBlock {
            break frame;
        }
    };
    let mut reader = neo_io::MemoryReader::new(&merkle_frame.payload_raw);
    let merkle =
        <neo_payloads::MerkleBlockPayload as neo_io::Serializable>::deserialize(&mut reader)
            .expect("decode merkle block");
    assert_eq!(merkle.tx_count, 1);
    assert_eq!(merkle.flags.first().copied().unwrap_or_default() & 1, 1);

    // Reload with only the transaction hash and exercise the GetBlockByIndex
    // path, which must share the same filtered response semantics.
    let empty_filter = BloomFilter::new(256, 3, 0).expect("empty bloom filter");
    let filter_load =
        neo_payloads::p2p_payloads::FilterLoadPayload::create_from_bloom_filter(&empty_filter);
    fake.send(
        Message::create(MessageCommand::FilterLoad, Some(&filter_load), false)
            .expect("encode second filterload"),
    )
    .await
    .expect("send second filterload");
    let filter_add =
        neo_payloads::p2p_payloads::FilterAddPayload::new(transaction.hash().to_bytes());
    fake.send(
        Message::create(MessageCommand::FilterAdd, Some(&filter_add), false)
            .expect("encode transaction filteradd"),
    )
    .await
    .expect("send transaction filteradd");
    let get_block = GetBlockByIndexPayload::create(0, 1);
    fake.send(
        Message::create(MessageCommand::GetBlockByIndex, Some(&get_block), false)
            .expect("encode getblockbyindex"),
    )
    .await
    .expect("send getblockbyindex");
    let merkle_frame = loop {
        let frame = recv_frame(&mut fake).await.expect("filtered index frame");
        if frame.command == MessageCommand::MerkleBlock {
            break frame;
        }
    };
    let mut reader = neo_io::MemoryReader::new(&merkle_frame.payload_raw);
    let merkle =
        <neo_payloads::MerkleBlockPayload as neo_io::Serializable>::deserialize(&mut reader)
            .expect("decode indexed merkle block");
    assert_eq!(merkle.tx_count, 1);
    assert_eq!(merkle.flags.first().copied().unwrap_or_default() & 1, 1);

    let clear = Message::from_payload_bytes(MessageCommand::FilterClear, Vec::new(), false)
        .expect("encode filterclear");
    fake.send(clear).await.expect("send filterclear");
    fake.send(
        Message::create(MessageCommand::GetData, Some(&request), false).expect("encode getdata"),
    )
    .await
    .expect("send unfiltered getdata");
    let block_frame = loop {
        let frame = recv_frame(&mut fake).await.expect("unfiltered block frame");
        if frame.command == MessageCommand::Block {
            break frame;
        }
    };
    let mut reader = neo_io::MemoryReader::new(&block_frame.payload_raw);
    <neo_payloads::Block as neo_io::Serializable>::deserialize(&mut reader)
        .expect("served full block round-trips");

    handle.shutdown().await.expect("shutdown");
}

/// The BloomFilter constructor rejects the otherwise wire-serializable empty
/// filter and zero-hash-function forms, so the session must close the peer.
#[tokio::test]
async fn node_rejects_invalid_filterload_bounds() {
    let (handle, mut fake) = local_node_with_block_source(0xfa4e_0011).await;
    let invalid = neo_payloads::p2p_payloads::FilterLoadPayload::new(Vec::new(), 0, 0);
    fake.send(
        Message::create(MessageCommand::FilterLoad, Some(&invalid), false)
            .expect("encode invalid filterload"),
    )
    .await
    .expect("send invalid filterload");
    expect_closed(&mut fake).await;
    handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn node_rejects_filterload_above_wire_limit() {
    let (handle, mut fake) = local_node_with_block_source(0xfa4e_0012).await;
    let mut writer = neo_io::BinaryWriter::new();
    writer
        .write_var_bytes(&vec![0u8; 36_001])
        .expect("encode oversized filter");
    writer.write_u8(1).expect("encode filter hash count");
    writer.write_u32(0).expect("encode filter tweak");
    let message =
        Message::from_payload_bytes(MessageCommand::FilterLoad, writer.into_bytes(), false)
            .expect("encode oversized filterload frame");
    fake.send(message).await.expect("send oversized filterload");
    expect_closed(&mut fake).await;
    handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn node_rejects_filteradd_above_wire_limit() {
    let (handle, mut fake) = local_node_with_block_source(0xfa4e_0013).await;
    let mut writer = neo_io::BinaryWriter::new();
    writer
        .write_var_bytes(&vec![0u8; 521])
        .expect("encode oversized filter item");
    let message =
        Message::from_payload_bytes(MessageCommand::FilterAdd, writer.into_bytes(), false)
            .expect("encode oversized filteradd frame");
    fake.send(message).await.expect("send oversized filteradd");
    expect_closed(&mut fake).await;
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
    let (service, handle) =
        LocalNodeService::with_config(test_chain_spec(), ChannelsConfig::default());
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
    let network = network_magic();
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
