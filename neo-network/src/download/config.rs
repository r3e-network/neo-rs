//! Downloader policy configuration.
//!
//! The config is source-neutral: sync drivers can use the same limits for P2P
//! peers, local package replay, or future state-sync sources.

use crate::PeerId;

/// Downloader policy for request scheduling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockDownloadConfig {
    /// Maximum number of in-flight peer requests.
    pub max_concurrency: usize,
    /// Maximum blocks yielded in one stream item.
    pub max_batch_size: usize,
    /// Number of times a failed request may be retried on another peer.
    pub retry_limit: usize,
    /// Preferred peer for biased requests.
    pub peer_bias: Option<PeerId>,
}

impl Default for BlockDownloadConfig {
    fn default() -> Self {
        Self {
            max_concurrency: 8,
            max_batch_size: 500,
            retry_limit: 2,
            peer_bias: None,
        }
    }
}

impl BlockDownloadConfig {
    /// Construct a config with clamped non-zero concurrency and batch size.
    #[must_use]
    pub fn new(max_concurrency: usize, max_batch_size: usize) -> Self {
        Self {
            max_concurrency: max_concurrency.max(1),
            max_batch_size: max_batch_size.max(1),
            ..Self::default()
        }
    }

    /// Override the retry limit.
    #[must_use]
    pub const fn with_retry_limit(mut self, retry_limit: usize) -> Self {
        self.retry_limit = retry_limit;
        self
    }

    /// Bias requests toward one peer.
    #[must_use]
    pub const fn with_peer_bias(mut self, peer_bias: PeerId) -> Self {
        self.peer_bias = Some(peer_bias);
        self
    }
}
