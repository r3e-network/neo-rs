use super::*;
use futures::StreamExt;
use neo_payloads::Block;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use crate::{NetworkError, NetworkResult, PeerId};

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
fn block_request_cap_matches_the_neo_inventory_limit() {
    assert_eq!(
        BlockRequest::MAX_COUNT,
        neo_payloads::inv_payload::MAX_HASHES_COUNT as u32
    );
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
fn block_range_scheduler_limits_each_peer_to_one_in_flight_range() {
    let peer_id = PeerId::from_raw(1);
    let config = BlockDownloadConfig::new(8, 64);
    let mut scheduler = CrossPeerBlockRangeScheduler::new(0, 128, config);
    let peers = [BlockDownloadPeer::new(peer_id, 128)];

    let first = scheduler.next_assignment(&peers).expect("first assignment");
    assert!(
        scheduler.next_assignment(&peers).is_none(),
        "one peer cannot serve overlapping correlated range fetches"
    );

    scheduler
        .record_success(first)
        .expect("complete first range");
    let second = scheduler
        .next_assignment(&peers)
        .expect("peer becomes eligible after completion");
    assert_eq!(second.peer_id, peer_id);
    assert_eq!(second.request.start, first.request.end().saturating_add(1));
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

#[derive(Clone, Default)]
struct FakeRangeFetcher {
    delays: Arc<parking_lot::Mutex<BTreeMap<u32, Duration>>>,
    fail_once: Arc<parking_lot::Mutex<BTreeSet<u32>>>,
    calls: Arc<parking_lot::Mutex<Vec<BlockRangeAssignment>>>,
}

impl FakeRangeFetcher {
    fn with_delay(self, start: u32, delay: Duration) -> Self {
        self.delays.lock().insert(start, delay);
        self
    }

    fn with_one_failure(self, start: u32) -> Self {
        self.fail_once.lock().insert(start);
        self
    }

    fn calls(&self) -> Vec<BlockRangeAssignment> {
        self.calls.lock().clone()
    }
}

impl BlockRangeFetcher for FakeRangeFetcher {
    fn fetch_range(
        &self,
        assignment: BlockRangeAssignment,
    ) -> impl std::future::Future<Output = NetworkResult<BlockDownloadBatch>> + Send + 'static {
        let fetcher = self.clone();
        async move {
            fetcher.calls.lock().push(assignment);
            let delay = fetcher
                .delays
                .lock()
                .get(&assignment.request.start)
                .copied();
            if let Some(delay) = delay {
                tokio::time::sleep(delay).await;
            }
            if fetcher.fail_once.lock().remove(&assignment.request.start) {
                return Err(NetworkError::Protocol(format!(
                    "temporary failure at {}",
                    assignment.request.start
                )));
            }
            let blocks = (assignment.request.start..=assignment.request.end())
                .map(block)
                .collect::<Vec<_>>();
            Ok(BlockDownloadBatch::new(
                Some(assignment.peer_id),
                assignment.request.start,
                blocks,
            ))
        }
    }
}

#[tokio::test]
async fn block_download_coordinator_yields_contiguous_batches_after_out_of_order_fetches() {
    let peer = PeerId::from_raw(1);
    let fetcher = FakeRangeFetcher::default().with_delay(1, Duration::from_millis(20));
    let mut coordinator = BlockDownloadCoordinator::new(
        0,
        4,
        vec![BlockDownloadPeer::new(peer, 4)],
        BlockDownloadConfig::new(2, 2),
        fetcher.clone(),
    );

    let first = coordinator
        .next()
        .await
        .expect("first batch")
        .expect("first ok");
    let second = coordinator
        .next()
        .await
        .expect("second batch")
        .expect("second ok");

    assert_eq!(first.start_height, 1);
    assert_eq!(first.next_height(), 3);
    assert_eq!(second.start_height, 3);
    assert_eq!(second.next_height(), 5);
    assert!(coordinator.next().await.is_none());
    assert_eq!(
        fetcher
            .calls()
            .into_iter()
            .map(|assignment| assignment.request.start)
            .collect::<Vec<_>>(),
        vec![1, 3]
    );
}

#[tokio::test]
async fn block_download_coordinator_retries_failed_range_on_another_peer() {
    let first_peer = PeerId::from_raw(1);
    let second_peer = PeerId::from_raw(2);
    let fetcher = FakeRangeFetcher::default().with_one_failure(1);
    let mut coordinator = BlockDownloadCoordinator::new(
        0,
        2,
        vec![
            BlockDownloadPeer::new(first_peer, 2),
            BlockDownloadPeer::new(second_peer, 2),
        ],
        BlockDownloadConfig::new(1, 2).with_retry_limit(1),
        fetcher.clone(),
    );

    let batch = coordinator
        .next()
        .await
        .expect("retry batch")
        .expect("retry ok");

    assert_eq!(batch.start_height, 1);
    assert_eq!(batch.peer_id, Some(second_peer));
    let calls = fetcher.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].peer_id, first_peer);
    assert_eq!(calls[0].attempt, 0);
    assert_eq!(calls[1].peer_id, second_peer);
    assert_eq!(calls[1].attempt, 1);
    assert!(coordinator.next().await.is_none());
}

#[tokio::test]
async fn block_download_coordinator_errors_when_no_peer_can_serve_next_range() {
    let peer = PeerId::from_raw(1);
    let mut coordinator = BlockDownloadCoordinator::new(
        0,
        4,
        vec![BlockDownloadPeer::new(peer, 0)],
        BlockDownloadConfig::new(1, 2),
        FakeRangeFetcher::default(),
    );

    let err = coordinator
        .next()
        .await
        .expect("stuck downloader should yield an error")
        .expect_err("no eligible peer");

    assert!(err.to_string().contains("no eligible peer"), "{err}");
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
