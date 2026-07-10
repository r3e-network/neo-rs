use super::*;

use neo_blockchain::{BlockchainCommand, BlockchainHandle};
use neo_network::{BlockDownloadBatch, BlockDownloadConfig, ChannelBlockDownloader, NetworkError};
use neo_payloads::{Block, Header};
use neo_runtime::{
    BlockImportQueue, BlockOrigin, CommitPolicy, InMemorySyncStageCheckpointStore,
    SyncStageCheckpoint, SyncStageCheckpointStore, SyncStageKind,
};
use std::sync::Arc;

fn block(index: u32) -> Block {
    let mut header = Header::new();
    header.set_index(index);
    Block::from_parts(header, vec![])
}

#[tokio::test]
async fn download_import_driver_drains_batches_into_canonical_import_queue() {
    let (blockchain, mut commands) = BlockchainHandle::with_capacity();
    let command_task = tokio::spawn(async move {
        let mut imported_batches = Vec::new();
        while let Some(command) = commands.recv().await {
            let BlockchainCommand::ImportBlocks { import, reply } = command else {
                panic!("unexpected blockchain command");
            };
            assert!(import.verify, "sync downloader imports should preverify");
            assert!(
                !import.bulk_sync,
                "sync downloader imports are not trusted local bulk replay"
            );
            let indexes = import.blocks.iter().map(Block::index).collect::<Vec<_>>();
            let imported = import.blocks.len();
            reply
                .send(neo_blockchain::ImportBlocksReply::ok(imported))
                .expect("send import reply");
            imported_batches.push(indexes);
            if imported_batches.len() == 2 {
                return imported_batches;
            }
        }
        panic!("blockchain command channel closed before both batches imported");
    });

    let queue = Arc::new(BlockImportQueue::new(Arc::new(blockchain), 2));
    let checkpoints = Arc::new(InMemorySyncStageCheckpointStore::default());
    let pipeline = Arc::new(SyncImportPipeline::with_parts(
        queue,
        checkpoints.clone(),
        CommitPolicy::new().with_max_blocks(2),
        BlockOrigin::Sync,
    ));
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
    assert_eq!(summary.checkpoints_written, 1);
    assert_eq!(
        summary.last_checkpoint,
        Some(SyncStageCheckpoint::new(SyncStageKind::Import, 2).with_counters(2, 0))
    );
    assert_eq!(
        checkpoints
            .checkpoint(SyncStageKind::Import)
            .expect("checkpoint read"),
        summary.last_checkpoint
    );

    assert_eq!(
        command_task.await.expect("command task"),
        vec![vec![1, 2], vec![3]]
    );
}

#[tokio::test]
async fn download_import_driver_surfaces_downloader_errors_before_import() {
    let (blockchain, _commands) = BlockchainHandle::with_capacity();
    let pipeline = Arc::new(SyncImportPipeline::with_parts(
        Arc::new(BlockImportQueue::new(Arc::new(blockchain), 2)),
        Arc::new(InMemorySyncStageCheckpointStore::default()),
        CommitPolicy::new().with_max_blocks(1),
        BlockOrigin::Sync,
    ));
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
async fn download_import_driver_allows_live_height_ahead_of_checkpoint() {
    let (blockchain, mut commands) = BlockchainHandle::with_capacity();
    let command_task = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            panic!("blockchain command channel closed before import");
        };
        let indexes = import.blocks.iter().map(Block::index).collect::<Vec<_>>();
        reply
            .send(neo_blockchain::ImportBlocksReply::ok(import.blocks.len()))
            .expect("send import reply");
        indexes
    });

    let checkpoints = Arc::new(InMemorySyncStageCheckpointStore::default());
    checkpoints
        .put_checkpoint(SyncStageCheckpoint::new(SyncStageKind::Import, 2).with_counters(2, 0))
        .expect("seed stale checkpoint");
    let pipeline = Arc::new(SyncImportPipeline::with_parts(
        Arc::new(BlockImportQueue::new(Arc::new(blockchain), 2)),
        checkpoints,
        CommitPolicy::new().with_max_blocks(1),
        BlockOrigin::Sync,
    ));
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
