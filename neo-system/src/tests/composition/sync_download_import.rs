use super::*;

use neo_blockchain::{BlockchainCommand, BlockchainHandle, HeaderCache, ImportMode};
use neo_network::{BlockDownloadBatch, BlockDownloadConfig, ChannelBlockDownloader, NetworkError};
use neo_payloads::{Block, Header};
use neo_runtime::{
    BlockImportQueue, BlockOrigin, CommitPolicy, InMemorySyncStageCheckpointStore,
    InMemoryVerifiedHeaderStore, SyncStageCheckpoint, SyncStageCheckpointStore, SyncStageKind,
    VerifiedHeaderStore,
};
use std::sync::Arc;

use crate::{StagedSyncPipeline, SyncHeaderPipeline, SyncImportPipeline};

fn block(index: u32) -> Block {
    let mut previous = neo_primitives::UInt256::zero();
    let mut block = Block::new();
    for height in 1..=index {
        let mut header = Header::new();
        header.set_index(height);
        header.set_prev_hash(previous);
        header.set_timestamp(u64::from(height) + 1);
        previous = header.hash();
        block = Block::from_parts(header, vec![]);
    }
    block
}

fn staged_pipeline(
    blockchain: BlockchainHandle,
    checkpoints: Arc<InMemorySyncStageCheckpointStore>,
    base_height: u32,
    target_height: u32,
    commit_policy: CommitPolicy,
) -> Arc<StagedSyncPipeline<InMemorySyncStageCheckpointStore, InMemoryVerifiedHeaderStore>> {
    let header_cache = Arc::new(HeaderCache::new());
    let header_store = Arc::new(InMemoryVerifiedHeaderStore::default());
    header_store
        .begin_window(base_height, target_height)
        .expect("begin header window");
    let headers = (base_height + 1..=target_height)
        .map(|height| block(height).header)
        .collect::<Vec<_>>();
    for header in &headers {
        assert!(header_cache.add(header.clone()));
    }
    header_store
        .commit_verified_headers(&headers)
        .expect("commit verified headers");
    let headers = Arc::new(SyncHeaderPipeline::new(
        blockchain.clone(),
        header_cache,
        header_store,
    ));
    let import = Arc::new(SyncImportPipeline::with_parts(
        Arc::new(BlockImportQueue::new(Arc::new(blockchain), 2)),
        checkpoints,
        commit_policy,
        BlockOrigin::Sync,
    ));
    Arc::new(StagedSyncPipeline::with_parts(headers, import))
}

#[tokio::test]
async fn download_import_driver_drains_batches_into_canonical_import_queue() {
    let (blockchain, mut commands) = BlockchainHandle::with_capacity();
    let command_task = tokio::spawn(async move {
        let mut imported_batches = Vec::new();
        while let Some(command) = commands.recv().await {
            match command {
                BlockchainCommand::ImportBlocks { import, reply } => {
                    assert_eq!(
                        import.mode,
                        ImportMode::Sync,
                        "coordinator batches need verified sync-batch semantics"
                    );
                    let indexes = import.blocks.iter().map(Block::index).collect::<Vec<_>>();
                    let imported = import.blocks.len();
                    reply
                        .send(neo_blockchain::ImportBlocksReply::ok(imported))
                        .expect("send import reply");
                    imported_batches.push(indexes);
                }
                BlockchainCommand::GetBlockByHeight { height, reply } => {
                    assert_eq!(height, 3);
                    let _ = reply.send(Some(block(height)));
                    return imported_batches;
                }
                other => panic!("unexpected blockchain command: {other:?}"),
            }
        }
        panic!("blockchain command channel closed before both batches imported");
    });

    let checkpoints = Arc::new(InMemorySyncStageCheckpointStore::default());
    let pipeline = staged_pipeline(
        blockchain,
        checkpoints.clone(),
        0,
        3,
        CommitPolicy::new().with_max_blocks(2),
    );
    let (tx, downloader) = ChannelBlockDownloader::channel(BlockDownloadConfig::new(2, 2), 4);
    tx.send(Ok(BlockDownloadBatch::new(
        None,
        1,
        vec![block(1), block(2)],
    )))
    .await
    .expect("send first batch");
    tx.send(Ok(BlockDownloadBatch::new(None, 3, vec![block(3)])))
        .await
        .expect("send second batch");
    drop(tx);

    let mut driver = SyncDownloadImportDriver::new(pipeline, downloader);
    let summary = driver.import_all().await.expect("import all downloads");

    assert_eq!(summary.downloaded_batches, 2);
    assert_eq!(summary.downloaded_blocks, 3);
    assert_eq!(summary.imported_blocks, 3);
    assert_eq!(summary.last_imported_height, Some(3));
    assert_eq!(summary.import_checkpoints_written, 1);
    assert_eq!(
        summary.last_import_checkpoint,
        Some(SyncStageCheckpoint::new(SyncStageKind::Import, 2).with_counters(2, 0))
    );
    assert_eq!(
        summary.body_checkpoint,
        Some(SyncStageCheckpoint::new(SyncStageKind::Bodies, 3).with_counters(3, 0))
    );
    assert_eq!(
        checkpoints
            .checkpoint(SyncStageKind::Import)
            .expect("checkpoint read"),
        summary.last_import_checkpoint
    );

    assert_eq!(
        command_task.await.expect("command task"),
        vec![vec![1, 2], vec![3]]
    );
}

#[tokio::test]
async fn download_import_driver_preserves_existing_checkpoint_until_commit_policy_fires() {
    let (blockchain, mut commands) = BlockchainHandle::with_capacity();
    let command_task = tokio::spawn(async move {
        let mut indexes = Vec::new();
        while let Some(command) = commands.recv().await {
            match command {
                BlockchainCommand::ImportBlocks { import, reply } => {
                    assert_eq!(import.mode, ImportMode::Sync);
                    indexes = import.blocks.iter().map(Block::index).collect::<Vec<_>>();
                    reply
                        .send(neo_blockchain::ImportBlocksReply::ok(import.blocks.len()))
                        .expect("send import reply");
                }
                BlockchainCommand::GetBlockByHeight { height, reply } => {
                    let _ = reply.send(Some(block(height)));
                    return indexes;
                }
                other => panic!("unexpected blockchain command: {other:?}"),
            }
        }
        indexes
    });

    let checkpoints = Arc::new(InMemorySyncStageCheckpointStore::default());
    let existing_checkpoint =
        SyncStageCheckpoint::new(SyncStageKind::Import, 2).with_counters(2, 0);
    checkpoints
        .put_checkpoint(existing_checkpoint.clone())
        .expect("seed prior checkpoint");
    let pipeline = staged_pipeline(
        blockchain,
        checkpoints.clone(),
        2,
        3,
        CommitPolicy::new().with_max_blocks(4),
    );
    let (tx, downloader) = ChannelBlockDownloader::channel(BlockDownloadConfig::new(1, 1), 1);
    tx.send(Ok(BlockDownloadBatch::new(None, 3, vec![block(3)])))
        .await
        .expect("send sync batch");
    drop(tx);

    let mut driver = SyncDownloadImportDriver::new(pipeline, downloader);
    let summary = driver
        .import_all()
        .await
        .expect("single batch below checkpoint threshold should still import");

    assert_eq!(summary.downloaded_batches, 1);
    assert_eq!(summary.imported_blocks, 1);
    assert_eq!(summary.last_imported_height, Some(3));
    assert_eq!(summary.import_checkpoints_written, 0);
    assert_eq!(summary.last_import_checkpoint, None);
    assert_eq!(
        checkpoints
            .checkpoint(SyncStageKind::Import)
            .expect("checkpoint read"),
        Some(existing_checkpoint),
        "the previous durable checkpoint must remain authoritative until the commit policy fires"
    );
    assert_eq!(command_task.await.expect("command task"), vec![3]);
}

#[tokio::test]
async fn download_import_driver_surfaces_downloader_errors_before_import() {
    let (blockchain, _commands) = BlockchainHandle::with_capacity();
    let header_store = Arc::new(InMemoryVerifiedHeaderStore::default());
    let headers = Arc::new(SyncHeaderPipeline::new(
        blockchain.clone(),
        Arc::new(HeaderCache::new()),
        header_store,
    ));
    let import = Arc::new(SyncImportPipeline::with_parts(
        Arc::new(BlockImportQueue::new(Arc::new(blockchain), 2)),
        Arc::new(InMemorySyncStageCheckpointStore::default()),
        CommitPolicy::new().with_max_blocks(1),
        BlockOrigin::Sync,
    ));
    let pipeline = Arc::new(StagedSyncPipeline::with_parts(headers, import));
    let (tx, downloader) = ChannelBlockDownloader::channel(BlockDownloadConfig::new(1, 1), 1);
    tx.send(Err(NetworkError::Protocol("download failed".to_string())))
        .await
        .expect("send error");
    drop(tx);

    let mut driver = SyncDownloadImportDriver::new(pipeline, downloader);
    let err = driver
        .import_all()
        .await
        .expect_err("download error must stop import");

    assert!(err.to_string().contains("download failed"), "{err}");
}

#[tokio::test]
async fn download_import_driver_never_imports_a_body_that_conflicts_with_verified_header() {
    let (blockchain, mut commands) = BlockchainHandle::with_capacity();
    let pipeline = staged_pipeline(
        blockchain,
        Arc::new(InMemorySyncStageCheckpointStore::default()),
        0,
        1,
        CommitPolicy::new().with_max_blocks(1),
    );
    let mut conflicting = block(1);
    conflicting.header.set_nonce(99);
    let (tx, downloader) = ChannelBlockDownloader::channel(BlockDownloadConfig::new(1, 1), 1);
    tx.send(Ok(BlockDownloadBatch::new(None, 1, vec![conflicting])))
        .await
        .expect("send conflicting body");
    drop(tx);

    let error = SyncDownloadImportDriver::new(pipeline, downloader)
        .import_all()
        .await
        .expect_err("verified-header mismatch must stop before canonical import");

    assert!(error.to_string().contains("does not match"), "{error}");
    assert!(
        commands.try_recv().is_err(),
        "no BlockchainCommand may be emitted for a mismatched body"
    );
}

#[tokio::test]
async fn download_import_driver_allows_live_height_ahead_of_checkpoint() {
    let (blockchain, mut commands) = BlockchainHandle::with_capacity();
    let command_task = tokio::spawn(async move {
        let mut indexes = Vec::new();
        while let Some(command) = commands.recv().await {
            match command {
                BlockchainCommand::ImportBlocks { import, reply } => {
                    indexes = import.blocks.iter().map(Block::index).collect::<Vec<_>>();
                    reply
                        .send(neo_blockchain::ImportBlocksReply::ok(import.blocks.len()))
                        .expect("send import reply");
                }
                BlockchainCommand::GetBlockByHeight { height, reply } => {
                    let _ = reply.send(Some(block(height)));
                    return indexes;
                }
                other => panic!("unexpected blockchain command: {other:?}"),
            }
        }
        indexes
    });

    let checkpoints = Arc::new(InMemorySyncStageCheckpointStore::default());
    checkpoints
        .put_checkpoint(SyncStageCheckpoint::new(SyncStageKind::Import, 2).with_counters(2, 0))
        .expect("seed stale checkpoint");
    let pipeline = staged_pipeline(
        blockchain,
        checkpoints,
        5,
        6,
        CommitPolicy::new().with_max_blocks(1),
    );
    let (tx, downloader) = ChannelBlockDownloader::channel(BlockDownloadConfig::new(1, 1), 1);
    tx.send(Ok(BlockDownloadBatch::new(None, 6, vec![block(6)])))
        .await
        .expect("send live batch");
    drop(tx);

    let mut driver = SyncDownloadImportDriver::new_at_chain_tip(pipeline, downloader, 5);
    let summary = driver
        .import_all()
        .await
        .expect("live-height sync should not be pinned to stale checkpoint");

    assert_eq!(summary.imported_blocks, 1);
    assert_eq!(summary.last_imported_height, Some(6));
    assert_eq!(command_task.await.expect("command task"), vec![6]);
}
