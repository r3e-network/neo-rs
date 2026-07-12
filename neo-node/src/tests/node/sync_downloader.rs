//! P2P sync downloader policy tests.

use super::{download_verified_headers, eligible_header_peers, p2p_staged_sync_config};
use crate::node::static_files::STATIC_ARCHIVE_MAX_DEFERRED_BLOCKS;
use futures::{SinkExt, StreamExt};
use neo_blockchain::{BlockchainCommand, BlockchainHandle, HeaderCache, HeaderValidationOutcome};
use neo_network::{
    LocalIdentity, Message, MessageCodec, MessageCommand, NetworkEvent, PeerRegistry,
    RemoteNodeHandle, RemoteNodeService, RemoteNodeState,
};
use neo_payloads::p2p_payloads::{GetBlockByIndexPayload, NodeCapability, VersionPayload};
use neo_payloads::{Header, HeadersPayload};
use neo_primitives::UInt256;
use neo_runtime::{
    InMemoryVerifiedHeaderStore, SyncStageCheckpointStore, SyncStageKind, VerifiedHeaderStore,
};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_util::codec::Framed;
use tokio_util::sync::CancellationToken;

type FakeFramed = Framed<TcpStream, MessageCodec>;

fn linked_headers(start: u32, count: u32, mut previous: UInt256) -> Vec<Header> {
    let mut headers = Vec::new();
    for index in start..start + count {
        let mut header = Header::new();
        header.set_index(index);
        header.set_prev_hash(previous);
        header.set_timestamp(u64::from(index) + 1);
        previous = header.hash();
        headers.push(header);
    }
    headers
}

async fn recv_frame(fake: &mut FakeFramed) -> Message {
    tokio::time::timeout(Duration::from_secs(2), fake.next())
        .await
        .expect("frame timed out")
        .expect("peer connection closed")
        .expect("decode frame")
}

async fn recv_getheaders(fake: &mut FakeFramed) -> GetBlockByIndexPayload {
    loop {
        let frame = recv_frame(fake).await;
        if frame.command == MessageCommand::GetHeaders {
            let mut reader = neo_io::MemoryReader::new(&frame.payload_raw);
            return <GetBlockByIndexPayload as neo_io::Serializable>::deserialize(&mut reader)
                .expect("decode GetHeaders");
        }
    }
}

async fn spawn_ready_peer(
    registry: Arc<PeerRegistry>,
    identity: Arc<LocalIdentity>,
    peer_id: neo_network::PeerId,
    remote_nonce: u32,
    height: u32,
) -> (FakeFramed, RemoteNodeHandle) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind peer");
    let address = listener.local_addr().expect("peer address");
    let fake_stream = TcpStream::connect(address).await.expect("connect peer");
    let (service_stream, remote_addr) = listener.accept().await.expect("accept peer");
    let (event_tx, _events) = broadcast::channel::<NetworkEvent>(16);
    let (service, handle) = RemoteNodeService::new(
        service_stream,
        peer_id,
        remote_addr,
        identity.clone(),
        Arc::clone(&registry),
        event_tx,
        RemoteNodeState::Handshake,
        CancellationToken::new(),
    );
    assert!(registry.try_admit(peer_id, remote_addr, handle.clone()));
    tokio::spawn(service.run());

    let mut fake = Framed::new(fake_stream, MessageCodec::new());
    assert_eq!(recv_frame(&mut fake).await.command, MessageCommand::Version);
    let version = VersionPayload::create(
        identity.network(),
        remote_nonce,
        "/staged-sync-test/".to_string(),
        height,
        vec![NodeCapability::full_node(height)],
    );
    fake.send(Message::create(MessageCommand::Version, Some(&version), false).expect("version"))
        .await
        .expect("send version");
    assert_eq!(recv_frame(&mut fake).await.command, MessageCommand::Verack);
    fake.send(
        Message::from_payload_bytes(MessageCommand::Verack, Vec::new(), false).expect("verack"),
    )
    .await
    .expect("send verack");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while !registry
        .download_peers()
        .iter()
        .any(|peer| peer.peer_id == peer_id)
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "peer did not become ready"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    (fake, handle)
}

#[test]
fn static_archive_bounds_downloaded_commit_batches() {
    let default = p2p_staged_sync_config(false);
    let bounded = p2p_staged_sync_config(true);

    assert_eq!(default, neo_network::BlockDownloadConfig::default());
    assert_eq!(bounded.max_batch_size, STATIC_ARCHIVE_MAX_DEFERRED_BLOCKS);
    assert_eq!(bounded.max_concurrency, default.max_concurrency);
    assert_eq!(bounded.retry_limit, default.retry_limit);
    assert_eq!(bounded.peer_bias, default.peer_bias);
}

#[test]
fn header_peer_selection_requires_range_coverage_and_excludes_failed_peers() {
    let low = neo_network::PeerId::new();
    let failed = neo_network::PeerId::new();
    let best = neo_network::PeerId::new();
    let peers = vec![
        neo_network::BlockDownloadPeer::new(low, 10),
        neo_network::BlockDownloadPeer::new(failed, 20),
        neo_network::BlockDownloadPeer::new(best, 30),
    ];
    let excluded = HashSet::from([failed]);

    let eligible = eligible_header_peers(peers, 15, &excluded);

    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].peer_id, best);
}

#[tokio::test]
async fn partial_header_prefix_retries_only_the_remaining_range_on_another_peer() {
    let valid = Arc::new(linked_headers(1, 3, UInt256::zero()));
    let cache = Arc::new(HeaderCache::new());
    let (blockchain, mut commands) = BlockchainHandle::with_capacity();
    let validator_cache = Arc::clone(&cache);
    let expected = Arc::clone(&valid);
    let validator = tokio::spawn(async move {
        while let Some(command) = commands.recv().await {
            match command {
                BlockchainCommand::ValidateHeaders { headers, reply } => {
                    let mut accepted = 0;
                    for header in headers {
                        let expected_header = expected
                            .get(header.index().saturating_sub(1) as usize)
                            .expect("header inside test target");
                        if header.hash() != expected_header.hash() {
                            break;
                        }
                        if validator_cache.hash_at(header.index()).is_none() {
                            assert!(validator_cache.add(header));
                        }
                        accepted += 1;
                    }
                    let _ = reply.send(HeaderValidationOutcome::new(
                        accepted,
                        validator_cache.last(),
                    ));
                }
                BlockchainCommand::Shutdown => break,
                other => panic!("unexpected blockchain command: {other:?}"),
            }
        }
    });
    let store = Arc::new(InMemoryVerifiedHeaderStore::default());
    let window = store.begin_window(0, 3).expect("begin header window");
    let progress = neo_system::HeaderStageProgress {
        window,
        checkpoint: store
            .checkpoint(SyncStageKind::Headers)
            .expect("checkpoint")
            .expect("Headers checkpoint"),
    };
    let pipeline = Arc::new(neo_system::SyncHeaderPipeline::new(
        blockchain.clone(),
        cache,
        Arc::clone(&store),
    ));

    let registry = Arc::new(PeerRegistry::with_limits(8, 8));
    let identity = Arc::new(LocalIdentity::new(
        neo_config::ProtocolSettings::default().network,
        7,
        "/neo-rs:staged-sync-test/".to_string(),
        true,
    ));
    let first_id = neo_network::PeerId::new();
    let second_id = neo_network::PeerId::new();
    let (mut first, first_handle) = spawn_ready_peer(
        Arc::clone(&registry),
        Arc::clone(&identity),
        first_id,
        0x1001,
        4,
    )
    .await;
    let (mut second, second_handle) =
        spawn_ready_peer(Arc::clone(&registry), identity, second_id, 0x1002, 3).await;

    let shutdown = CancellationToken::new();
    let download = tokio::spawn(async move {
        download_verified_headers(pipeline, registry, progress, 1, &shutdown).await
    });
    let first_request = recv_getheaders(&mut first).await;
    assert_eq!(first_request.index_start, 1);
    assert_eq!(first_request.count, 3);
    let mut invalid_suffix = valid[1].clone();
    invalid_suffix.set_nonce(99);
    let response = HeadersPayload::create(vec![valid[0].clone(), invalid_suffix]);
    first
        .send(Message::create(MessageCommand::Headers, Some(&response), false).expect("headers"))
        .await
        .expect("send partial response");

    let second_request = recv_getheaders(&mut second).await;
    assert_eq!(second_request.index_start, 2);
    assert_eq!(second_request.count, 2);
    let response = HeadersPayload::create(vec![valid[1].clone(), valid[2].clone()]);
    second
        .send(Message::create(MessageCommand::Headers, Some(&response), false).expect("headers"))
        .await
        .expect("send remaining response");

    let completed = download
        .await
        .expect("download task joined")
        .expect("second peer completes header window");
    assert!(completed.is_complete());
    assert_eq!(completed.checkpoint.height, 3);
    for (height, expected) in (1..=3).zip(valid.iter()) {
        let stored = store
            .header(height)
            .expect("read durable header")
            .expect("durable header exists");
        assert_eq!(stored.hash(), expected.hash());
    }

    first_handle.shutdown().await.expect("shutdown first peer");
    second_handle
        .shutdown()
        .await
        .expect("shutdown second peer");
    blockchain.shutdown().await.expect("shutdown validator");
    validator.await.expect("validator task");
}
