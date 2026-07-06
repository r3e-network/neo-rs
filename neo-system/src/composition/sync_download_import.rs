//! Download-stream to import-pipeline bridge.
//!
//! This composition helper owns no peer scheduling and no block validation
//! rules. It consumes any `neo-network` downloader stream, converts downloaded
//! batches into runtime sync batches, and delegates import/checkpoint semantics
//! to [`SyncImportPipeline`].

use std::pin::Pin;
use std::sync::Arc;

use neo_network::BlockDownloader;
use neo_runtime::{ServiceResult, SyncPipelineImportOutcome, SyncStageCheckpoint};

use crate::sync_import_pipeline::SyncImportPipeline;

/// Aggregate result for draining a downloader into the sync import pipeline.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SyncDownloadImportSummary {
    /// Download batches consumed from the downloader stream.
    pub downloaded_batches: u64,
    /// Blocks carried by consumed download batches.
    pub downloaded_blocks: u64,
    /// Blocks reported as processed by the canonical import path.
    pub imported_blocks: u64,
    /// Import-stage checkpoints written while draining the stream.
    pub checkpoints_written: u64,
    /// Highest block height reported as imported, when at least one block was processed.
    pub last_imported_height: Option<u32>,
    /// Last checkpoint written while draining the stream.
    pub last_checkpoint: Option<SyncStageCheckpoint>,
}

impl SyncDownloadImportSummary {
    fn record(&mut self, downloaded_blocks: usize, outcome: SyncPipelineImportOutcome) {
        self.downloaded_batches = self.downloaded_batches.saturating_add(1);
        self.downloaded_blocks = self
            .downloaded_blocks
            .saturating_add(u64::try_from(downloaded_blocks).unwrap_or(u64::MAX));
        self.imported_blocks = self
            .imported_blocks
            .saturating_add(u64::try_from(outcome.imported.processed).unwrap_or(u64::MAX));
        if outcome.imported.processed > 0 {
            self.last_imported_height = outcome.next_height.map(|height| height.saturating_sub(1));
        }
        if let Some(checkpoint) = outcome.checkpoint {
            self.checkpoints_written = self.checkpoints_written.saturating_add(1);
            self.last_checkpoint = Some(checkpoint);
        }
    }
}

/// Bridges a block downloader stream into the node-composed sync import pipeline.
pub struct SyncDownloadImportDriver<D: BlockDownloader> {
    pipeline: Arc<SyncImportPipeline>,
    downloader: D,
}

impl<D: BlockDownloader> SyncDownloadImportDriver<D> {
    /// Create a driver over the shared sync import pipeline and a downloader.
    #[must_use]
    pub fn new(pipeline: Arc<SyncImportPipeline>, downloader: D) -> Self {
        Self {
            pipeline,
            downloader,
        }
    }

    /// Returns the sync import pipeline used by this driver.
    #[must_use]
    pub fn pipeline(&self) -> Arc<SyncImportPipeline> {
        Arc::clone(&self.pipeline)
    }

    /// Returns the downloader scheduling config.
    #[must_use]
    pub fn downloader_config(&self) -> &neo_network::BlockDownloadConfig {
        self.downloader.config()
    }

    /// Drain every available download batch into the canonical import pipeline.
    ///
    /// Import remains ordered by [`neo_runtime::SyncPipelineDriver`]; this
    /// method only converts and forwards batches. A downloader error or a
    /// partial import result stops the loop and returns the corresponding
    /// runtime service error.
    pub async fn import_all(&mut self) -> ServiceResult<SyncDownloadImportSummary> {
        let mut import_driver = self.pipeline.driver()?;
        let mut summary = SyncDownloadImportSummary::default();

        while let Some(downloaded) = next_download_batch(&mut self.downloader).await {
            let batch = downloaded.map_err(neo_runtime::ServiceError::from)?;
            let downloaded_blocks = batch.blocks.len();
            let outcome = import_driver.push_batch(batch.into()).await?;
            summary.record(downloaded_blocks, outcome);
        }

        Ok(summary)
    }
}

async fn next_download_batch<D: BlockDownloader>(
    downloader: &mut D,
) -> Option<neo_network::NetworkResult<neo_network::BlockDownloadBatch>> {
    std::future::poll_fn(|cx| {
        neo_network::BlockDownloader::poll_next_batch(Pin::new(&mut *downloader), cx)
    })
    .await
}

#[cfg(test)]
#[path = "../tests/composition/sync_download_import.rs"]
mod tests;
