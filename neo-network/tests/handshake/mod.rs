//! # neo-network::tests::handshake
//!
//! Test module grouping handshake behavior coverage for neo-network.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-network; it may assemble fixtures
//! but must not introduce production behavior.
//!
//! ## Contents
//!
//! - `block_source`: test block-source fixtures.
//! - `handshake_flow`: P2P handshake flow coverage.
//! - `inventory`: inventory payload traits and records.
//! - `limits`: network limit coverage.
//! - `rejections`: handshake rejection coverage.
//! - `sync_runtime`: sync runtime test harness.

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_util::codec::Framed;
use tokio_util::sync::CancellationToken;

use neo_config::ProtocolSettings;
use neo_network::MessageCommand;
use neo_network::wire::{Message, MessageCodec};
use neo_network::{
    ChannelsConfig, ConnectionTimeouts, InboundInventory, LocalIdentity, LocalNodeService,
    NetworkEvent, NetworkHandle, PeerId, PeerRegistry, RemoteNodeService, RemoteNodeState,
};
use neo_payloads::p2p_payloads::{
    GetBlockByIndexPayload, InvPayload, NodeCapability, PingPayload, VersionPayload,
};
use neo_primitives::{InventoryType, UInt256};

/// Generous upper bound for every await in these tests; real
/// exchanges complete in milliseconds.
const TEST_TIMEOUT: Duration = Duration::from_secs(10);

type FakeFramed = Framed<TcpStream, MessageCodec>;

async fn fake_dial(port: u16) -> FakeFramed {
    let stream = tokio::time::timeout(TEST_TIMEOUT, TcpStream::connect(("127.0.0.1", port)))
        .await
        .expect("dial timed out")
        .expect("dial failed");
    Framed::new(stream, MessageCodec::new())
}

/// Read the next frame, panicking on timeout. Returns `None` when the
/// remote closed the connection.
async fn recv_frame(framed: &mut FakeFramed) -> Option<Message> {
    match tokio::time::timeout(TEST_TIMEOUT, framed.next()).await {
        Ok(Some(Ok(message))) => Some(message),
        Ok(Some(Err(err))) => panic!("frame decode failed: {err}"),
        Ok(None) => None,
        Err(_) => panic!("timed out waiting for a frame"),
    }
}

/// Assert the connection is closed from the fake peer's perspective:
/// the next read yields EOF (or a reset, which dropping an aborted
/// socket may surface as an error).
async fn expect_closed(framed: &mut FakeFramed) {
    match tokio::time::timeout(TEST_TIMEOUT, framed.next()).await {
        Ok(None) | Ok(Some(Err(_))) => {}
        Ok(Some(Ok(message))) => panic!(
            "expected the connection to be closed, received {:?}",
            message.command
        ),
        Err(_) => panic!("timed out waiting for the connection to close"),
    }
}

fn version_message(network: u32, nonce: u32, listener_port: u16) -> Message {
    let mut capabilities = vec![NodeCapability::full_node(0)];
    if listener_port > 0 {
        capabilities.push(NodeCapability::tcp_server(listener_port));
    }
    let payload = VersionPayload::create(
        network,
        nonce,
        "/fake-peer:0.0.1/".to_string(),
        0,
        capabilities,
    );
    Message::create(MessageCommand::Version, Some(&payload), false).expect("encode version")
}

fn verack_message() -> Message {
    Message::from_payload_bytes(MessageCommand::Verack, Vec::new(), false).expect("encode verack")
}

fn decode_version(message: &Message) -> VersionPayload {
    assert_eq!(message.command, MessageCommand::Version);
    let mut reader = neo_io::MemoryReader::new(&message.payload_raw);
    <VersionPayload as neo_io::Serializable>::deserialize(&mut reader).expect("decode version")
}

/// Await the next event matching `pred`, panicking on timeout.
async fn await_event<F>(events: &mut broadcast::Receiver<NetworkEvent>, mut pred: F) -> NetworkEvent
where
    F: FnMut(&NetworkEvent) -> bool,
{
    loop {
        let event = tokio::time::timeout(TEST_TIMEOUT, events.recv())
            .await
            .expect("timed out waiting for a network event")
            .expect("event channel open");
        if pred(&event) {
            return event;
        }
    }
}

/// Poll `handle.local_node_info()` until `pred` holds, panicking on
/// timeout. Used for assertions on the handle-side `getpeers` fold.
async fn await_info<F>(handle: &NetworkHandle, mut pred: F)
where
    F: FnMut(&neo_network::handle::LocalNodeInfo) -> bool,
{
    let deadline = tokio::time::Instant::now() + TEST_TIMEOUT;
    loop {
        if pred(&handle.local_node_info()) {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for the handle-side peer view to converge"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Start a `LocalNodeService` bound to an ephemeral port, returning
/// `(handle, events, listen_port)`. The events receiver is subscribed
/// before the listener starts so no lifecycle event can be missed.
async fn start_local_node(
    config: ChannelsConfig,
) -> (NetworkHandle, broadcast::Receiver<NetworkEvent>, u16) {
    let settings = Arc::new(ProtocolSettings::default());
    let (service, handle) = LocalNodeService::with_config(settings, config);
    tokio::spawn(service.run());
    let events = handle.subscribe();
    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");
    let port = handle.local_node_info().port();
    assert_ne!(port, 0);
    (handle, events, port)
}

async fn start_local_node_with_seeded_height(
    config: ChannelsConfig,
    height: u32,
) -> (NetworkHandle, broadcast::Receiver<NetworkEvent>, u16) {
    let settings = Arc::new(ProtocolSettings::default());
    let (service, handle) = LocalNodeService::with_config(settings, config);
    tokio::spawn(service.run());
    let events = handle.subscribe();
    handle
        .set_block_height(height)
        .await
        .expect("seed local height before listener starts");
    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");
    let port = handle.local_node_info().port();
    assert_ne!(port, 0);
    (handle, events, port)
}

/// Drive the fake-peer side of a full handshake: read the node's
/// version, reply with our own, read the verack, reply with verack.
/// Returns the node's version payload.
async fn complete_handshake(
    framed: &mut FakeFramed,
    network: u32,
    nonce: u32,
    listener_port: u16,
) -> VersionPayload {
    let node_version = recv_frame(framed).await.expect("node version");
    let node_version = decode_version(&node_version);
    framed
        .send(version_message(network, nonce, listener_port))
        .await
        .expect("send version");
    let verack = recv_frame(framed).await.expect("verack");
    assert_eq!(verack.command, MessageCommand::Verack);
    framed.send(verack_message()).await.expect("send verack");
    node_version
}

async fn recv_getblockbyindex(fake: &mut FakeFramed) -> GetBlockByIndexPayload {
    loop {
        let frame = recv_frame(fake).await.expect("getblockbyindex frame");
        if frame.command == MessageCommand::GetBlockByIndex {
            let mut reader = neo_io::MemoryReader::new(&frame.payload_raw);
            return <GetBlockByIndexPayload as neo_io::Serializable>::deserialize(&mut reader)
                .expect("decode getblockbyindex");
        }
    }
}

#[path = "block_source.rs"]
mod block_source;
#[path = "handshake_flow.rs"]
mod handshake_flow;
#[path = "inventory.rs"]
mod inventory;
#[path = "limits.rs"]
mod limits;
#[path = "rejections.rs"]
mod rejections;
#[path = "sync_runtime.rs"]
mod sync_runtime;
