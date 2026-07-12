//! Download-stream to import-pipeline bridge.
//!
//! This composition helper owns no peer scheduling or protocol validation
//! rules. It consumes a body downloader stream, applies the composed durable
//! header gate, converts matching batches into runtime sync batches, and
//! delegates canonical import/checkpoint semantics to [`StagedSyncPipeline`].

use std::pin::Pin;
use std::sync::Arc;

use neo_network::BlockDownloader;
use neo_runtime::{
    ServiceResult, SharedStoreSyncStageCheckpointStore, SharedStoreVerifiedHeaderStore,
    SyncPipelineImportOutcome, SyncStageCheckpoint, SyncStageCheckpointStore, VerifiedHeaderStore,
};

use crate::staged_sync_pipeline::StagedSyncPipeline;

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
    pub import_checkpoints_written: u64,
    /// Highest block height reported as imported, when at least one block was processed.
    pub last_imported_height: Option<u32>,
    /// Last import-stage checkpoint written while draining the stream.
    pub last_import_checkpoint: Option<SyncStageCheckpoint>,
    /// Bodies-stage checkpoint written after the fixed target became canonical.
    pub body_checkpoint: Option<SyncStageCheckpoint>,
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
            self.import_checkpoints_written = self.import_checkpoints_written.saturating_add(1);
            self.last_import_checkpoint = Some(checkpoint);
        }
    }
}

/// Bridges a block downloader stream into the node-composed sync import pipeline.
pub struct SyncDownloadImportDriver<
    D: BlockDownloader,
    C: SyncStageCheckpointStore = SharedStoreSyncStageCheckpointStore,
    H: VerifiedHeaderStore = SharedStoreVerifiedHeaderStore,
> {
    pipeline: Arc<StagedSyncPipeline<C, H>>,
    downloader: D,
    chain_tip_height: Option<u32>,
}

impl<D, C, H> SyncDownloadImportDriver<D, C, H>
where
    D: BlockDownloader,
    C: SyncStageCheckpointStore,
    H: VerifiedHeaderStore,
{
    /// Create a driver over the shared sync import pipeline and a downloader.
    #[must_use]
    pub fn new(pipeline: Arc<StagedSyncPipeline<C, H>>, downloader: D) -> Self {
        Self {
            pipeline,
            downloader,
            chain_tip_height: None,
        }
    }

    /// Create a driver whose first downloader batch starts after `chain_tip_height`.
    ///
    /// This is the production P2P-sync constructor. It keeps the reusable
    /// runtime sync driver strict, but aligns that driver's initial cursor with
    /// the node's authoritative local tip before draining the downloader.
    #[must_use]
    pub fn new_at_chain_tip(
        pipeline: Arc<StagedSyncPipeline<C, H>>,
        downloader: D,
        chain_tip_height: u32,
    ) -> Self {
        Self {
            pipeline,
            downloader,
            chain_tip_height: Some(chain_tip_height),
        }
    }

    /// Returns the sync import pipeline used by this driver.
    #[must_use]
    pub fn pipeline(&self) -> Arc<StagedSyncPipeline<C, H>> {
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
        let import_pipeline = self.pipeline.import();
        let mut import_driver = import_pipeline.driver()?;
        if let Some(chain_tip_height) = self.chain_tip_height {
            import_driver.align_next_height_to_chain_tip(chain_tip_height);
        }
        let mut summary = SyncDownloadImportSummary::default();

        while let Some(downloaded) = next_download_batch(&mut self.downloader).await {
            let batch = downloaded.map_err(neo_runtime::ServiceError::from)?;
            self.pipeline.headers().verify_body_batch(&batch)?;
            let downloaded_blocks = batch.blocks.len();
            let outcome = import_driver.push_batch(batch.into()).await?;
            summary.record(downloaded_blocks, outcome);
        }

        if let Some(height) = summary.last_imported_height {
            summary.body_checkpoint = self
                .pipeline
                .headers()
                .finish_imported_window(height)
                .await?;
            if summary.body_checkpoint.is_none() {
                return Err(neo_runtime::ServiceError::invalid_state(
                    "body import completed without an active header-stage window",
                ));
            }
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
#[path = "../../tests/composition/sync_download_import.rs"]
mod tests;
