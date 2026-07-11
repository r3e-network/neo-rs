//! P2P sync downloader policy tests.

use super::p2p_block_download_config;
use crate::node::static_files::STATIC_ARCHIVE_MAX_DEFERRED_BLOCKS;

#[test]
fn static_archive_bounds_downloaded_commit_batches() {
    let default = p2p_block_download_config(false);
    let bounded = p2p_block_download_config(true);

    assert_eq!(default, neo_network::BlockDownloadConfig::default());
    assert_eq!(bounded.max_batch_size, STATIC_ARCHIVE_MAX_DEFERRED_BLOCKS);
    assert_eq!(bounded.max_concurrency, default.max_concurrency);
    assert_eq!(bounded.retry_limit, default.retry_limit);
    assert_eq!(bounded.peer_bias, default.peer_bias);
}
