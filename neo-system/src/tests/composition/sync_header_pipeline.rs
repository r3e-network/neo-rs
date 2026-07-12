use super::*;

use neo_blockchain::{BlockchainCommand, HeaderValidationOutcome};
use neo_network::{BlockDownloadBatch, HeaderDownloadBatch};
use neo_payloads::{Block, Header};
use neo_primitives::UInt256;
use neo_runtime::{
    InMemoryVerifiedHeaderStore, SyncStageCheckpointStore, SyncStageKind, VerifiedHeaderStore,
};

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

fn validation_handle(cache: Arc<HeaderCache>) -> (BlockchainHandle, tokio::task::JoinHandle<()>) {
    let (blockchain, mut commands) = BlockchainHandle::with_capacity();
    let task = tokio::spawn(async move {
        while let Some(command) = commands.recv().await {
            match command {
                BlockchainCommand::ValidateHeaders { headers, reply } => {
                    for header in &headers {
                        if cache.hash_at(header.index()).is_none() {
                            assert!(cache.add(header.clone()));
                        }
                    }
                    let frontier = headers.last().cloned();
                    let _ = reply.send(HeaderValidationOutcome::new(headers.len(), frontier));
                }
                BlockchainCommand::Shutdown => break,
                other => panic!("unexpected blockchain command: {other:?}"),
            }
        }
    });
    (blockchain, task)
}

#[tokio::test]
async fn accepted_header_prefix_becomes_durable_body_gate() {
    let cache = Arc::new(HeaderCache::new());
    let (blockchain, task) = validation_handle(Arc::clone(&cache));
    let store = Arc::new(InMemoryVerifiedHeaderStore::default());
    store.begin_window(0, 2).expect("begin window");
    let pipeline = SyncHeaderPipeline::new(blockchain.clone(), cache, Arc::clone(&store));
    let headers = linked_headers(1, 2, UInt256::zero());

    let outcome = pipeline
        .accept_downloaded_headers(HeaderDownloadBatch::new(None, 1, headers.clone()))
        .await
        .expect("validate and commit headers");

    assert_eq!(outcome.received, 2);
    assert_eq!(outcome.accepted, 2);
    assert_eq!(outcome.rejected(), 0);
    assert!(outcome.progress.is_complete());
    assert_eq!(
        store
            .checkpoint(SyncStageKind::Headers)
            .expect("checkpoint")
            .expect("headers checkpoint")
            .height,
        2
    );

    let matching = BlockDownloadBatch::new(
        None,
        1,
        headers
            .iter()
            .cloned()
            .map(|header| Block::from_parts(header, Vec::new()))
            .collect(),
    );
    pipeline
        .verify_body_batch(&matching)
        .expect("matching bodies pass the gate");

    let mut conflicting = headers[0].clone();
    conflicting.set_nonce(99);
    let mismatch =
        BlockDownloadBatch::new(None, 1, vec![Block::from_parts(conflicting, Vec::new())]);
    let error = pipeline
        .verify_body_batch(&mismatch)
        .expect_err("conflicting body must not reach import");
    assert!(error.to_string().contains("does not match"), "{error}");

    blockchain.shutdown().await.expect("shutdown validator");
    task.await.expect("validator task");
}

#[tokio::test]
async fn prepare_window_rehydrates_durable_headers_and_keeps_fixed_target() {
    let store = Arc::new(InMemoryVerifiedHeaderStore::default());
    let headers = linked_headers(6, 2, UInt256::zero());
    store.begin_window(5, 8).expect("begin window");
    store
        .commit_verified_headers(&headers)
        .expect("seed durable headers");

    let cache = Arc::new(HeaderCache::new());
    let (blockchain, mut commands) = BlockchainHandle::with_capacity();
    let responder_cache = Arc::clone(&cache);
    let task = tokio::spawn(async move {
        while let Some(command) = commands.recv().await {
            match command {
                BlockchainCommand::GetHeight { reply } => {
                    let _ = reply.send(5);
                }
                BlockchainCommand::ValidateHeaders { headers, reply } => {
                    for header in &headers {
                        assert!(responder_cache.add(header.clone()));
                    }
                    let frontier = headers.last().cloned();
                    let _ = reply.send(HeaderValidationOutcome::new(headers.len(), frontier));
                }
                BlockchainCommand::Shutdown => break,
                other => panic!("unexpected blockchain command: {other:?}"),
            }
        }
    });
    let pipeline = SyncHeaderPipeline::new(blockchain.clone(), Arc::clone(&cache), store);

    let progress = pipeline
        .prepare_window(20)
        .await
        .expect("recover window")
        .expect("active window");

    assert_eq!(progress.window.target_height, 8, "target stays fixed");
    assert_eq!(progress.checkpoint.height, 7);
    assert_eq!(cache.hash_at(6), Some(headers[0].hash()));
    assert_eq!(cache.hash_at(7), Some(headers[1].hash()));

    blockchain.shutdown().await.expect("shutdown responder");
    task.await.expect("responder task");
}

#[tokio::test]
async fn divergent_canonical_tip_resets_durable_and_in_memory_header_views() {
    let store = Arc::new(InMemoryVerifiedHeaderStore::default());
    let staged = linked_headers(6, 1, UInt256::zero());
    store.begin_window(5, 8).expect("begin window");
    store
        .commit_verified_headers(&staged)
        .expect("seed staged header");
    let cache = Arc::new(HeaderCache::new());
    assert!(cache.add(staged[0].clone()));

    let mut canonical_header = staged[0].clone();
    canonical_header.set_nonce(77);
    let canonical = Block::from_parts(canonical_header, Vec::new());
    let (blockchain, mut commands) = BlockchainHandle::with_capacity();
    let task = tokio::spawn(async move {
        while let Some(command) = commands.recv().await {
            match command {
                BlockchainCommand::GetHeight { reply } => {
                    let _ = reply.send(6);
                }
                BlockchainCommand::GetBlockByHeight { height, reply } => {
                    assert_eq!(height, 6);
                    let _ = reply.send(Some(canonical.clone()));
                }
                BlockchainCommand::Shutdown => break,
                other => panic!("unexpected blockchain command: {other:?}"),
            }
        }
    });
    let pipeline =
        SyncHeaderPipeline::new(blockchain.clone(), Arc::clone(&cache), Arc::clone(&store));

    let progress = pipeline
        .prepare_window(20)
        .await
        .expect("reset divergent window")
        .expect("replacement window");

    assert_eq!(progress.window.base_height, 6);
    assert_eq!(progress.window.target_height, 8, "fixed target is retained");
    assert_eq!(progress.checkpoint.height, 6);
    assert_eq!(cache.count(), 0, "stale process-local headers are cleared");
    assert!(store.header(6).expect("header read").is_none());

    blockchain.shutdown().await.expect("shutdown responder");
    task.await.expect("responder task");
}

#[tokio::test]
async fn consumed_window_recovery_is_idempotent_after_bodies_checkpoint() {
    let store = Arc::new(InMemoryVerifiedHeaderStore::default());
    let headers = linked_headers(6, 2, UInt256::zero());
    store.begin_window(5, 7).expect("begin window");
    store
        .commit_verified_headers(&headers)
        .expect("complete verified window");
    let bodies = SyncStageCheckpoint::new(SyncStageKind::Bodies, 7).with_counters(2, 99);
    store
        .put_checkpoint(bodies.clone())
        .expect("seed checkpoint written before sidecar prune");

    let canonical = Block::from_parts(headers[1].clone(), Vec::new());
    let (blockchain, mut commands) = BlockchainHandle::with_capacity();
    let task = tokio::spawn(async move {
        while let Some(command) = commands.recv().await {
            match command {
                BlockchainCommand::GetHeight { reply } => {
                    let _ = reply.send(7);
                }
                BlockchainCommand::GetBlockByHeight { height, reply } => {
                    assert_eq!(height, 7);
                    let _ = reply.send(Some(canonical.clone()));
                }
                BlockchainCommand::Shutdown => break,
                other => panic!("unexpected blockchain command: {other:?}"),
            }
        }
    });
    let pipeline = SyncHeaderPipeline::new(
        blockchain.clone(),
        Arc::new(HeaderCache::new()),
        Arc::clone(&store),
    );

    assert_eq!(
        pipeline
            .prepare_window(7)
            .await
            .expect("reconcile consumed window"),
        None
    );
    assert_eq!(store.window().expect("window after prune"), None);
    assert!(store.header(6).expect("header 6 after prune").is_none());
    assert!(store.header(7).expect("header 7 after prune").is_none());
    assert_eq!(
        store
            .checkpoint(SyncStageKind::Bodies)
            .expect("Bodies checkpoint after recovery"),
        Some(bodies),
        "recovery must not count the already-checkpointed range twice"
    );

    blockchain.shutdown().await.expect("shutdown responder");
    task.await.expect("responder task");
}

#[tokio::test]
async fn consumed_incomplete_window_does_not_advance_bodies_checkpoint() {
    let store = Arc::new(InMemoryVerifiedHeaderStore::default());
    store.begin_window(5, 7).expect("begin incomplete window");
    let bodies = SyncStageCheckpoint::new(SyncStageKind::Bodies, 4).with_counters(4, 11);
    store
        .put_checkpoint(bodies.clone())
        .expect("seed prior Bodies checkpoint");

    let (blockchain, mut commands) = BlockchainHandle::with_capacity();
    let task = tokio::spawn(async move {
        while let Some(command) = commands.recv().await {
            match command {
                BlockchainCommand::GetHeight { reply } => {
                    let _ = reply.send(7);
                }
                BlockchainCommand::Shutdown => break,
                other => panic!("unexpected blockchain command: {other:?}"),
            }
        }
    });
    let pipeline = SyncHeaderPipeline::new(
        blockchain.clone(),
        Arc::new(HeaderCache::new()),
        Arc::clone(&store),
    );

    assert_eq!(
        pipeline
            .prepare_window(7)
            .await
            .expect("discard incomplete consumed window"),
        None
    );
    assert_eq!(store.window().expect("window after discard"), None);
    assert_eq!(
        store
            .checkpoint(SyncStageKind::Bodies)
            .expect("Bodies checkpoint after discard"),
        Some(bodies),
        "an incomplete header target cannot claim body-stage completion"
    );

    blockchain.shutdown().await.expect("shutdown responder");
    task.await.expect("responder task");
}

#[tokio::test]
async fn consumed_divergent_target_does_not_advance_bodies_checkpoint() {
    let store = Arc::new(InMemoryVerifiedHeaderStore::default());
    let headers = linked_headers(6, 2, UInt256::zero());
    store.begin_window(5, 7).expect("begin window");
    store
        .commit_verified_headers(&headers)
        .expect("complete verified window");
    let bodies = SyncStageCheckpoint::new(SyncStageKind::Bodies, 5).with_counters(5, 12);
    store
        .put_checkpoint(bodies.clone())
        .expect("seed prior Bodies checkpoint");

    let mut conflicting = headers[1].clone();
    conflicting.set_nonce(77);
    let canonical = Block::from_parts(conflicting, Vec::new());
    let (blockchain, mut commands) = BlockchainHandle::with_capacity();
    let task = tokio::spawn(async move {
        while let Some(command) = commands.recv().await {
            match command {
                BlockchainCommand::GetHeight { reply } => {
                    let _ = reply.send(7);
                }
                BlockchainCommand::GetBlockByHeight { height, reply } => {
                    assert_eq!(height, 7);
                    let _ = reply.send(Some(canonical.clone()));
                }
                BlockchainCommand::Shutdown => break,
                other => panic!("unexpected blockchain command: {other:?}"),
            }
        }
    });
    let pipeline = SyncHeaderPipeline::new(
        blockchain.clone(),
        Arc::new(HeaderCache::new()),
        Arc::clone(&store),
    );

    let error = pipeline
        .prepare_window(7)
        .await
        .expect_err("divergent fixed target must be rejected");
    assert!(error.to_string().contains("fixed header target"), "{error}");
    assert_eq!(store.window().expect("window after rejection"), None);
    assert_eq!(
        store
            .checkpoint(SyncStageKind::Bodies)
            .expect("Bodies checkpoint after rejection"),
        Some(bodies),
        "a divergent target cannot claim body-stage completion"
    );

    blockchain.shutdown().await.expect("shutdown responder");
    task.await.expect("responder task");
}
