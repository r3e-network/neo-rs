use super::*;
use futures::{SinkExt, StreamExt};
use neo_network::MessageCommand;
use neo_network::wire::{Message, MessageCodec};
use neo_payloads::p2p_payloads::{GetBlockByIndexPayload, NodeCapability, VersionPayload};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_util::codec::Framed;

const TEST_TIMEOUT: Duration = Duration::from_secs(10);

type FakeFramed = Framed<TcpStream, MessageCodec>;

async fn fake_dial(port: u16) -> FakeFramed {
    let stream = tokio::time::timeout(TEST_TIMEOUT, TcpStream::connect(("127.0.0.1", port)))
        .await
        .expect("dial timed out")
        .expect("dial failed");
    Framed::new(stream, MessageCodec::new())
}

async fn recv_frame(framed: &mut FakeFramed) -> Message {
    match tokio::time::timeout(TEST_TIMEOUT, framed.next()).await {
        Ok(Some(Ok(message))) => message,
        Ok(Some(Err(err))) => panic!("frame decode failed: {err}"),
        Ok(None) => panic!("connection closed while waiting for frame"),
        Err(_) => panic!("timed out waiting for frame"),
    }
}

fn decode_payload<T: neo_io::Serializable>(message: &Message) -> T {
    let mut reader = neo_io::MemoryReader::new(&message.payload_raw);
    <T as neo_io::Serializable>::deserialize(&mut reader).expect("decode payload")
}

fn fake_peer_version_message(network: u32, nonce: u32, height: u32) -> Message {
    let payload = VersionPayload::create(
        network,
        nonce,
        "/fake-peer:0.0.1/".to_string(),
        vec![
            NodeCapability::full_node(height),
            NodeCapability::tcp_server(20333),
        ],
    );
    Message::create(MessageCommand::Version, Some(&payload), false).expect("encode version")
}

fn verack_message() -> Message {
    Message::from_payload_bytes(MessageCommand::Verack, Vec::new(), false).expect("encode verack")
}

async fn recv_getblockbyindex(fake: &mut FakeFramed) -> GetBlockByIndexPayload {
    loop {
        let frame = recv_frame(fake).await;
        if frame.command == MessageCommand::GetBlockByIndex {
            return decode_payload(&frame);
        }
    }
}

fn empty_child_block(parent: &neo_payloads::Block, index: u32) -> neo_payloads::Block {
    let mut header = neo_payloads::Header::new();
    header.set_index(index);
    header.set_prev_hash(parent.hash());
    header.set_timestamp(parent.header.timestamp() + 15_000);
    header.set_next_consensus(*parent.header.next_consensus());
    header.witness =
        neo_payloads::Witness::new_with_scripts(Vec::new(), vec![neo_vm_rs::OpCode::PUSH1.byte()]);
    neo_payloads::Block::from_parts(header, Vec::new())
}

fn seed_rocksdb_tip(path: &Path, settings: &ProtocolSettings, tip: u32) -> anyhow::Result<()> {
    use neo_storage::persistence::StoreCache;

    neo_native_contracts::install();

    let config = NodeConfig::default();
    let store = open_store(&config, Some(path))?;
    let mut store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());

    let mut parent = Arc::new(neo_blockchain::genesis_block(settings)?);
    neo_blockchain::persist_block_natives(Arc::clone(&snapshot), Arc::clone(&parent), settings)?;

    for index in 1..=tip {
        let block = Arc::new(empty_child_block(parent.as_ref(), index));
        neo_blockchain::persist_block_natives(Arc::clone(&snapshot), Arc::clone(&block), settings)?;
        parent = block;
    }

    let current_index = neo_native_contracts::LedgerContract::new()
        .current_index(&snapshot)
        .expect("seeded ledger current index");
    assert_eq!(current_index, tip);
    store_cache
        .try_commit()
        .map_err(|err| anyhow::anyhow!("commit seeded RocksDB store: {err}"))?;

    Ok(())
}

fn unused_local_rpc_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral RPC port");
    listener.local_addr().expect("local RPC address").port()
}

async fn rpc_post_json(port: u16, request: serde_json::Value) -> serde_json::Value {
    let client = reqwest::Client::new();
    let response = tokio::time::timeout(
        TEST_TIMEOUT,
        client
            .post(format!("http://127.0.0.1:{port}/"))
            .json(&request)
            .send(),
    )
    .await
    .expect("RPC request timed out")
    .expect("RPC request failed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    response
        .json::<serde_json::Value>()
        .await
        .expect("parse RPC response JSON")
}

mod config_parsing;
mod config_validation;
mod runtime;
