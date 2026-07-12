use super::*;

use neo_blockchain::{BlockchainHandle, HeaderCache};
use neo_network::{BlockRequest, PeerId};
use neo_payloads::{Block, Header};
use neo_primitives::UInt256;
use neo_runtime::{InMemoryVerifiedHeaderStore, VerifiedHeaderStore};

#[derive(Clone)]
struct FixedFetcher {
    batch: BlockDownloadBatch,
}

impl BlockRangeFetcher for FixedFetcher {
    fn fetch_range(
        &self,
        _assignment: BlockRangeAssignment,
    ) -> impl std::future::Future<Output = NetworkResult<BlockDownloadBatch>> + Send + 'static {
        let batch = self.batch.clone();
        async move { Ok(batch) }
    }
}

fn header(index: u32, previous: UInt256) -> Header {
    let mut header = Header::new();
    header.set_index(index);
    header.set_prev_hash(previous);
    header
}

#[tokio::test]
async fn mismatch_fails_inside_fetch_for_coordinator_retry() {
    let expected = header(1, UInt256::zero());
    let cache = Arc::new(HeaderCache::new());
    assert!(cache.add(expected.clone()));
    let store = Arc::new(InMemoryVerifiedHeaderStore::default());
    store.begin_window(0, 1).expect("begin window");
    store
        .commit_verified_headers(std::slice::from_ref(&expected))
        .expect("commit header");
    let (blockchain, _commands) = BlockchainHandle::with_capacity();
    let headers = Arc::new(SyncHeaderPipeline::new(blockchain, cache, store));

    let mut conflicting = expected;
    conflicting.set_nonce(7);
    let fetcher = VerifiedBlockRangeFetcher::new(
        FixedFetcher {
            batch: BlockDownloadBatch::new(
                None,
                1,
                vec![Block::from_parts(conflicting, Vec::new())],
            ),
        },
        headers,
    );
    let assignment = BlockRangeAssignment::new(PeerId::new(), BlockRequest::new(1, 1), 0);

    let error = fetcher
        .fetch_range(assignment)
        .await
        .expect_err("body mismatch must be a peer-range failure");
    assert!(error.to_string().contains("invalid body range"), "{error}");
    assert!(error.to_string().contains("does not match"), "{error}");
}
