use super::*;
use futures::StreamExt;
use neo_payloads::Block;

use crate::PeerId;

fn block(index: u32) -> Block {
    let mut header = neo_payloads::Header::new();
    header.set_index(index);
    Block::from_parts(header, vec![])
}

#[test]
fn block_download_config_clamps_concurrency_and_batch_size() {
    let config = BlockDownloadConfig::new(0, 0).with_retry_limit(4);

    assert_eq!(config.max_concurrency, 1);
    assert_eq!(config.max_batch_size, 1);
    assert_eq!(config.retry_limit, 4);
    assert!(config.peer_bias.is_none());
}

#[test]
fn block_download_config_records_peer_bias() {
    let peer = PeerId::new();
    let config = BlockDownloadConfig::new(16, 500).with_peer_bias(peer);

    assert_eq!(config.max_concurrency, 16);
    assert_eq!(config.max_batch_size, 500);
    assert_eq!(config.peer_bias, Some(peer));
}

#[test]
fn block_download_batch_converts_to_runtime_sync_batch() {
    let batch = BlockDownloadBatch::new(None, 7, vec![block(7), block(8)]);

    let sync_batch: neo_runtime::SyncBlockBatch = batch.into();

    assert_eq!(sync_batch.start_height, 7);
    assert_eq!(sync_batch.next_height(), 9);
    assert_eq!(
        sync_batch
            .blocks
            .iter()
            .map(Block::index)
            .collect::<Vec<_>>(),
        vec![7, 8]
    );
}

#[test]
fn block_request_scheduler_requests_two_protocol_windows() {
    let mut scheduler = BlockRequestScheduler::default();

    let first = scheduler.next_request(0, 5_000).expect("first request");
    let second = scheduler.next_request(0, 5_000).expect("second request");
    let third = scheduler.next_request(0, 5_000);

    assert_eq!(first, BlockRequest::new(1, 500));
    assert_eq!(second, BlockRequest::new(501, 500));
    assert!(third.is_none());
}

#[test]
fn block_request_scheduler_resumes_from_persisted_tip() {
    let mut scheduler = BlockRequestScheduler::default();
    scheduler
        .next_request(42, 100)
        .expect("request after durable tip");

    assert_eq!(scheduler.requested_to(), 100);
}

#[test]
fn block_request_scheduler_resets_when_caught_up() {
    let mut scheduler = BlockRequestScheduler::default();
    scheduler.next_request(0, 100).expect("request");

    assert!(scheduler.next_request(100, 100).is_none());
    assert_eq!(scheduler.requested_to(), 100);
    assert_eq!(scheduler.stall_ticks(), 0);
}

#[test]
fn block_request_scheduler_rewinds_after_stall_limit() {
    let mut scheduler = BlockRequestScheduler::default();
    scheduler.next_request(0, 5_000).expect("first");
    scheduler.next_request(0, 5_000).expect("second");

    for _ in 0..BlockRequestScheduler::STALL_LIMIT {
        scheduler.record_tick(0, 5_000);
    }

    let retry = scheduler.next_request(0, 5_000).expect("retry after stall");
    assert_eq!(retry, BlockRequest::new(1, 500));
}

#[test]
fn block_range_scheduler_retries_failed_range_on_another_peer() {
    let first_peer = PeerId::from_raw(1);
    let second_peer = PeerId::from_raw(2);
    let config = BlockDownloadConfig::new(1, 64).with_retry_limit(1);
    let mut scheduler = CrossPeerBlockRangeScheduler::new(0, 128, config);
    let peers = [
        BlockDownloadPeer::new(first_peer, 128),
        BlockDownloadPeer::new(second_peer, 128),
    ];

    let first = scheduler.next_assignment(&peers).expect("first assignment");
    scheduler
        .record_failure(first)
        .expect("failure below retry limit");
    let retry = scheduler.next_assignment(&peers).expect("retry assignment");

    assert_eq!(retry.request, first.request);
    assert_eq!(retry.peer_id, second_peer);
    assert_eq!(retry.attempt, 1);
}

#[test]
fn ordered_block_batch_buffer_holds_out_of_order_until_gap_fills() {
    let mut buffer = OrderedBlockBatchBuffer::new(1);

    buffer
        .insert(BlockDownloadBatch::new(None, 3, vec![block(3), block(4)]))
        .expect("insert out-of-order batch");
    assert!(buffer.pop_ready().is_none());

    buffer
        .insert(BlockDownloadBatch::new(None, 1, vec![block(1), block(2)]))
        .expect("insert first batch");
    let first = buffer.pop_ready().expect("first ready batch");
    let second = buffer.pop_ready().expect("second ready batch");

    assert_eq!(first.start_height, 1);
    assert_eq!(first.next_height(), 3);
    assert_eq!(second.start_height, 3);
    assert_eq!(second.next_height(), 5);
    assert_eq!(buffer.next_height(), 5);
}

#[test]
fn ordered_block_batch_buffer_rejects_misaligned_block_indices() {
    let mut buffer = OrderedBlockBatchBuffer::new(1);

    let result = buffer.insert(BlockDownloadBatch::new(None, 1, vec![block(2)]));

    assert!(result.is_err());
    assert!(buffer.pop_ready().is_none());
}

#[tokio::test]
async fn channel_block_downloader_streams_batches_in_send_order() {
    let config = BlockDownloadConfig::new(2, 3);
    let (tx, mut downloader) = ChannelBlockDownloader::channel(config, 2);
    assert_eq!(downloader.config(), &config);

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

    let first = downloader
        .next()
        .await
        .expect("first item")
        .expect("first batch");
    let second = downloader
        .next()
        .await
        .expect("second item")
        .expect("second batch");

    assert_eq!(first.start_height, 1);
    assert_eq!(first.next_height(), 3);
    assert_eq!(second.start_height, 3);
    assert_eq!(second.next_height(), 4);
    assert!(downloader.next().await.is_none());
}
