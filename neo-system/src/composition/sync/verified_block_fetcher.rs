//! Header-gated block-range fetch adapter.
//!
//! This statically dispatched adapter keeps peer retry effective: a body that
//! disagrees with the verified header chain fails inside the fetch future, so
//! `BlockDownloadCoordinator` can reassign that exact range to another peer.

use std::sync::Arc;

use neo_network::{
    BlockDownloadBatch, BlockRangeAssignment, BlockRangeFetcher, NetworkError, NetworkResult,
};
use neo_runtime::VerifiedHeaderStore;

use crate::sync_header_pipeline::SyncHeaderPipeline;

/// Block-range fetcher that rejects responses outside the verified header chain.
#[derive(Debug)]
pub struct VerifiedBlockRangeFetcher<F, H>
where
    F: BlockRangeFetcher,
    H: VerifiedHeaderStore,
{
    inner: F,
    headers: Arc<SyncHeaderPipeline<H>>,
}

impl<F, H> Clone for VerifiedBlockRangeFetcher<F, H>
where
    F: BlockRangeFetcher,
    H: VerifiedHeaderStore,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            headers: Arc::clone(&self.headers),
        }
    }
}

impl<F, H> VerifiedBlockRangeFetcher<F, H>
where
    F: BlockRangeFetcher,
    H: VerifiedHeaderStore,
{
    /// Wrap a transport fetcher with the durable header-stage gate.
    #[must_use]
    pub fn new(inner: F, headers: Arc<SyncHeaderPipeline<H>>) -> Self {
        Self { inner, headers }
    }

    /// Returns the wrapped transport fetcher.
    #[must_use]
    pub const fn inner(&self) -> &F {
        &self.inner
    }
}

impl<F, H> BlockRangeFetcher for VerifiedBlockRangeFetcher<F, H>
where
    F: BlockRangeFetcher,
    H: VerifiedHeaderStore + 'static,
{
    fn fetch_range(
        &self,
        assignment: BlockRangeAssignment,
    ) -> impl std::future::Future<Output = NetworkResult<BlockDownloadBatch>> + Send + 'static {
        let inner = self.inner.clone();
        let headers = Arc::clone(&self.headers);
        async move {
            let batch = inner.fetch_range(assignment).await?;
            headers.verify_body_batch(&batch).map_err(|error| {
                NetworkError::Protocol(format!(
                    "peer {} returned an invalid body range {}..={}: {error}",
                    assignment.peer_id,
                    assignment.request.start,
                    assignment.request.end()
                ))
            })?;
            Ok(batch)
        }
    }
}

#[cfg(test)]
#[path = "../../tests/composition/verified_block_fetcher.rs"]
mod tests;
