//! Durable header-stage composition and body/header gating.
//!
//! This module binds the blockchain service's single header verifier to a
//! durable [`neo_runtime::VerifiedHeaderStore`]. It owns recovery and the
//! bounded in-memory `HeaderCache` acceleration view, but it never writes
//! canonical Ledger rows or imports blocks.

use std::sync::Arc;

use neo_blockchain::{BlockchainHandle, HeaderCache};
use neo_network::{BlockDownloadBatch, HeaderDownloadBatch, PeerId};
use neo_payloads::{Block, Header};
use neo_primitives::UInt256;
use neo_runtime::{
    HeaderStageWindow, MAX_VERIFIED_HEADER_WINDOW, ServiceError, ServiceResult,
    SyncStageCheckpoint, SyncStageKind, VerifiedHeaderStore,
};

const RECOVERY_VALIDATION_BATCH: usize = 2_000;

/// Durable progress for one fixed header-stage window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeaderStageProgress {
    /// Canonical anchor and fixed target.
    pub window: HeaderStageWindow,
    /// Highest durably materialized verified header.
    pub checkpoint: SyncStageCheckpoint,
}

impl HeaderStageProgress {
    /// Height to request next, or one past the target when complete.
    #[must_use]
    pub const fn next_height(&self) -> u32 {
        self.checkpoint.height.saturating_add(1)
    }

    /// Returns whether the fixed target and its hash are durable.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.checkpoint.height == self.window.target_height && self.window.target_hash.is_some()
    }
}

/// Result of validating and durably staging one peer header response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeaderStageBatchOutcome {
    /// Peer that supplied the response, when known.
    pub peer_id: Option<PeerId>,
    /// Headers carried by the response.
    pub received: usize,
    /// Valid prefix durably committed from the response.
    pub accepted: usize,
    /// Resulting durable header-stage progress.
    pub progress: HeaderStageProgress,
}

impl HeaderStageBatchOutcome {
    /// Headers rejected from the response suffix.
    #[must_use]
    pub fn rejected(&self) -> usize {
        self.received.saturating_sub(self.accepted)
    }
}

/// Node-composed durable `Headers` stage.
pub struct SyncHeaderPipeline<H: VerifiedHeaderStore> {
    blockchain: BlockchainHandle,
    header_cache: Arc<HeaderCache>,
    store: Arc<H>,
}

impl<H: VerifiedHeaderStore> std::fmt::Debug for SyncHeaderPipeline<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncHeaderPipeline")
            .field("blockchain", &self.blockchain)
            .field("cached_headers", &self.header_cache.count())
            .finish_non_exhaustive()
    }
}

impl<H: VerifiedHeaderStore> SyncHeaderPipeline<H> {
    /// Compose the canonical header verifier, shared cache, and durable sidecar.
    #[must_use]
    pub fn new(
        blockchain: BlockchainHandle,
        header_cache: Arc<HeaderCache>,
        store: Arc<H>,
    ) -> Self {
        Self {
            blockchain,
            header_cache,
            store,
        }
    }

    /// Returns the durable verified-header provider.
    #[must_use]
    pub fn store(&self) -> Arc<H> {
        Arc::clone(&self.store)
    }

    /// Returns the shared in-memory verified-header view.
    #[must_use]
    pub fn header_cache(&self) -> Arc<HeaderCache> {
        Arc::clone(&self.header_cache)
    }

    /// Reconcile durable sidecar state with the canonical tip and prepare one
    /// bounded fixed-target sync window.
    pub async fn prepare_window(
        &self,
        proposed_target: u32,
    ) -> ServiceResult<Option<HeaderStageProgress>> {
        let canonical_height = self.blockchain.get_height().await?;
        let bounded_target =
            proposed_target.min(canonical_height.saturating_add(MAX_VERIFIED_HEADER_WINDOW));

        if let Some(active) = self.store.window()? {
            if canonical_height >= active.target_height {
                self.reconcile_consumed_window(canonical_height, &active)
                    .await?;
            } else if canonical_height < active.base_height {
                return self.reset_or_finish(canonical_height, bounded_target).await;
            } else {
                return self.recover_active_window(canonical_height, active).await;
            }
        }

        if bounded_target <= canonical_height {
            return Ok(None);
        }
        self.header_cache.clear();
        let window = self.store.begin_window(canonical_height, bounded_target)?;
        self.progress_for(window).map(Some)
    }

    /// Validate one correlated peer response through `neo-blockchain`, then
    /// atomically stage only its accepted prefix with the `Headers` checkpoint.
    pub async fn accept_downloaded_headers(
        &self,
        batch: HeaderDownloadBatch,
    ) -> ServiceResult<HeaderStageBatchOutcome> {
        if batch.is_empty() {
            return Err(ServiceError::invalid_input(
                "header stage received an empty correlated response",
            ));
        }
        let before = self.current_progress()?;
        if batch.start_height != before.next_height() {
            return Err(ServiceError::invalid_input(format!(
                "header response starts at {}, expected {}",
                batch.start_height,
                before.next_height()
            )));
        }
        let received = batch.headers.len();
        neo_runtime::sync_metrics::record_headers_downloaded(
            u64::try_from(received).unwrap_or(u64::MAX),
        );

        let validation = self
            .blockchain
            .validate_headers(batch.headers.clone())
            .await?;
        if validation.accepted > received {
            return Err(ServiceError::invalid_state(format!(
                "header validator accepted {} of {received} headers",
                validation.accepted
            )));
        }

        let checkpoint = if validation.accepted == 0 {
            before.checkpoint.clone()
        } else {
            let accepted = &batch.headers[..validation.accepted];
            self.ensure_reported_frontier(validation.frontier.as_ref(), accepted)?;
            self.store.commit_verified_headers(accepted)?
        };
        if validation.accepted > 0 {
            neo_runtime::sync_metrics::record_headers_verified(
                u64::try_from(validation.accepted).unwrap_or(u64::MAX),
                u64::from(checkpoint.height),
            );
        }

        let progress = self.current_progress()?;
        Ok(HeaderStageBatchOutcome {
            peer_id: batch.peer_id,
            received,
            accepted: validation.accepted,
            progress,
        })
    }

    /// Reject any body batch that exceeds or disagrees with the durable
    /// verified-header frontier.
    pub fn verify_body_batch(&self, batch: &BlockDownloadBatch) -> ServiceResult<()> {
        if batch.is_empty() {
            return Err(ServiceError::invalid_input(
                "bodies stage received an empty block batch",
            ));
        }
        let progress = self.current_progress()?;
        let end_height = batch
            .start_height
            .checked_add(
                u32::try_from(batch.blocks.len())
                    .map_err(|_| ServiceError::invalid_input("body batch length exceeds u32"))?,
            )
            .and_then(|height| height.checked_sub(1))
            .ok_or_else(|| ServiceError::invalid_input("body batch height overflow"))?;
        if end_height > progress.checkpoint.height || end_height > progress.window.target_height {
            return Err(ServiceError::invalid_input(format!(
                "body batch ends at {end_height}, beyond verified header frontier {}",
                progress.checkpoint.height
            )));
        }

        for (offset, block) in batch.blocks.iter().enumerate() {
            let expected_height =
                batch
                    .start_height
                    .checked_add(u32::try_from(offset).map_err(|_| {
                        ServiceError::invalid_input("body batch offset exceeds u32")
                    })?)
                    .ok_or_else(|| ServiceError::invalid_input("body batch height overflow"))?;
            if block.index() != expected_height {
                return Err(ServiceError::invalid_input(format!(
                    "body batch expected block {expected_height}, got {}",
                    block.index()
                )));
            }
            let expected_hash = self.verified_hash_at(expected_height)?;
            let actual_hash = hash_block(block)?;
            if actual_hash != expected_hash {
                neo_runtime::sync_metrics::record_body_header_mismatch();
                return Err(ServiceError::invalid_input(format!(
                    "block {expected_height} hash does not match verified header"
                )));
            }
        }
        Ok(())
    }

    /// Mark a fully imported fixed window complete, persist the `Bodies`
    /// checkpoint, and prune sidecar headers now owned by canonical Ledger.
    pub async fn finish_imported_window(
        &self,
        canonical_height: u32,
    ) -> ServiceResult<Option<SyncStageCheckpoint>> {
        let Some(window) = self.store.window()? else {
            return Ok(None);
        };
        if canonical_height < window.target_height {
            return Err(ServiceError::invalid_input(format!(
                "cannot finish body stage at {canonical_height} before target {}",
                window.target_height
            )));
        }
        let progress = self.progress_for(window.clone())?;
        if !progress.is_complete() {
            return Err(ServiceError::invalid_state(
                "cannot finish body stage before the verified header target is complete",
            ));
        }
        self.ensure_canonical_target(&window).await?;

        let checkpoint = self.checkpoint_bodies(&window, canonical_height)?;
        self.store.finish_window(canonical_height)?;
        neo_runtime::sync_metrics::record_bodies_checkpoint(u64::from(canonical_height));
        Ok(Some(checkpoint))
    }

    fn current_progress(&self) -> ServiceResult<HeaderStageProgress> {
        let window = self
            .store
            .window()?
            .ok_or_else(|| ServiceError::invalid_state("no active header stage window"))?;
        self.progress_for(window)
    }

    fn progress_for(&self, window: HeaderStageWindow) -> ServiceResult<HeaderStageProgress> {
        let checkpoint = self
            .store
            .checkpoint(SyncStageKind::Headers)?
            .ok_or_else(|| ServiceError::invalid_state("active header window has no checkpoint"))?;
        if checkpoint.height < window.base_height || checkpoint.height > window.target_height {
            return Err(ServiceError::invalid_state(format!(
                "Headers checkpoint {} is outside active window {}..={}",
                checkpoint.height, window.base_height, window.target_height
            )));
        }
        if checkpoint.height == window.target_height && window.target_hash.is_none() {
            return Err(ServiceError::invalid_state(
                "verified-header target hash is missing after the target height was reached",
            ));
        }
        if checkpoint.height < window.target_height && window.target_hash.is_some() {
            return Err(ServiceError::invalid_state(
                "verified-header target hash is present before the target height was reached",
            ));
        }
        Ok(HeaderStageProgress { window, checkpoint })
    }

    async fn recover_active_window(
        &self,
        canonical_height: u32,
        window: HeaderStageWindow,
    ) -> ServiceResult<Option<HeaderStageProgress>> {
        let progress = match self.progress_for(window.clone()) {
            Ok(progress) => progress,
            Err(error) => {
                self.reset_window(canonical_height, window.target_height)?;
                return Err(ServiceError::invalid_state(format!(
                    "reset invalid Headers checkpoint at canonical height {canonical_height}: {error}"
                )));
            }
        };
        if progress.checkpoint.height < canonical_height {
            let reset = self.reset_window(canonical_height, window.target_height)?;
            return self.progress_for(reset).map(Some);
        }

        if canonical_height > window.base_height {
            let canonical_hash = self.canonical_hash(canonical_height).await?;
            let staged_hash = self
                .store
                .header(canonical_height)?
                .map(|header| header.try_hash())
                .transpose()
                .map_err(|error| {
                    ServiceError::invalid_state(format!(
                        "hash staged header {canonical_height}: {error}"
                    ))
                })?;
            if staged_hash != Some(canonical_hash) {
                let reset = self.reset_window(canonical_height, window.target_height)?;
                return self.progress_for(reset).map(Some);
            }
        }

        let mut next = canonical_height.saturating_add(1);
        while next <= progress.checkpoint.height {
            let end = progress
                .checkpoint
                .height
                .min(next.saturating_add(RECOVERY_VALIDATION_BATCH as u32 - 1));
            let mut headers = Vec::with_capacity((end - next + 1) as usize);
            for height in next..=end {
                let header = match self.store.header(height) {
                    Ok(Some(header)) => header,
                    Ok(None) => {
                        let reset = self.reset_window(canonical_height, window.target_height)?;
                        return self.progress_for(reset).map(Some);
                    }
                    Err(error) => {
                        self.reset_window(canonical_height, window.target_height)?;
                        return Err(ServiceError::invalid_state(format!(
                            "discarded corrupt staged header {height}: {error}"
                        )));
                    }
                };
                headers.push(header);
            }
            let outcome = self.blockchain.validate_headers(headers).await?;
            if outcome.accepted != (end - next + 1) as usize {
                let reset = self.reset_window(canonical_height, window.target_height)?;
                return self.progress_for(reset).map(Some);
            }
            next = end.saturating_add(1);
        }
        Ok(Some(self.current_progress()?))
    }

    async fn reconcile_consumed_window(
        &self,
        canonical_height: u32,
        window: &HeaderStageWindow,
    ) -> ServiceResult<()> {
        let progress = match self.progress_for(window.clone()) {
            Ok(progress) => progress,
            Err(error) => {
                self.header_cache.clear();
                self.store.discard_window(canonical_height)?;
                return Err(ServiceError::invalid_state(format!(
                    "discarded invalid consumed header window at canonical height {canonical_height}: {error}"
                )));
            }
        };
        if !progress.is_complete() {
            self.header_cache.clear();
            self.store.discard_window(canonical_height)?;
            return Ok(());
        }
        if let Err(error) = self.ensure_canonical_target(window).await {
            self.header_cache.clear();
            self.store.discard_window(canonical_height)?;
            return Err(error);
        }

        self.checkpoint_bodies(window, canonical_height)?;
        self.store.finish_window(canonical_height)?;
        neo_runtime::sync_metrics::record_bodies_checkpoint(u64::from(canonical_height));
        Ok(())
    }

    fn checkpoint_bodies(
        &self,
        window: &HeaderStageWindow,
        canonical_height: u32,
    ) -> ServiceResult<SyncStageCheckpoint> {
        let previous = self.store.checkpoint(SyncStageKind::Bodies)?;
        let previous_height = previous.as_ref().map_or(window.base_height, |checkpoint| {
            checkpoint.height.max(window.base_height)
        });
        let processed = previous
            .as_ref()
            .map_or(0, |checkpoint| checkpoint.processed_blocks)
            .saturating_add(u64::from(canonical_height.saturating_sub(previous_height)));
        let changed_bytes = previous
            .as_ref()
            .map_or(0, |checkpoint| checkpoint.changed_bytes);
        let checkpoint = SyncStageCheckpoint::new(SyncStageKind::Bodies, canonical_height)
            .with_counters(processed, changed_bytes);
        self.store.put_checkpoint(checkpoint.clone())?;
        Ok(checkpoint)
    }

    async fn reset_or_finish(
        &self,
        canonical_height: u32,
        bounded_target: u32,
    ) -> ServiceResult<Option<HeaderStageProgress>> {
        self.header_cache.clear();
        self.store.discard_window(canonical_height)?;
        if bounded_target <= canonical_height {
            return Ok(None);
        }
        let window = self.store.begin_window(canonical_height, bounded_target)?;
        self.progress_for(window).map(Some)
    }

    fn reset_window(
        &self,
        canonical_height: u32,
        target_height: u32,
    ) -> ServiceResult<HeaderStageWindow> {
        self.header_cache.clear();
        self.store.reset_window(canonical_height, target_height)
    }

    fn ensure_reported_frontier(
        &self,
        frontier: Option<&Header>,
        accepted: &[Header],
    ) -> ServiceResult<()> {
        let expected = accepted.last().ok_or_else(|| {
            ServiceError::invalid_state("nonzero header acceptance has no accepted header")
        })?;
        let reported = frontier.ok_or_else(|| {
            ServiceError::invalid_state("header validator omitted the accepted frontier")
        })?;
        if reported.index() < expected.index()
            || self.header_cache.hash_at(expected.index()) != Some(expected.hash())
        {
            return Err(ServiceError::invalid_state(
                "header validator reported a frontier outside its accepted prefix",
            ));
        }
        Ok(())
    }

    fn verified_hash_at(&self, height: u32) -> ServiceResult<UInt256> {
        if let Some(hash) = self.header_cache.hash_at(height) {
            return Ok(hash);
        }
        let header = self.store.header(height)?.ok_or_else(|| {
            ServiceError::invalid_state(format!(
                "verified header {height} is absent from cache and durable sidecar"
            ))
        })?;
        header.try_hash().map_err(|error| {
            ServiceError::invalid_state(format!("hash verified header {height}: {error}"))
        })
    }

    async fn ensure_canonical_target(&self, window: &HeaderStageWindow) -> ServiceResult<()> {
        let expected = window.target_hash.ok_or_else(|| {
            ServiceError::invalid_state("completed header window has no target hash")
        })?;
        let actual = self.canonical_hash(window.target_height).await?;
        if actual != expected {
            return Err(ServiceError::invalid_state(format!(
                "canonical block {} does not match the fixed header target",
                window.target_height
            )));
        }
        Ok(())
    }

    async fn canonical_hash(&self, height: u32) -> ServiceResult<UInt256> {
        let block = self
            .blockchain
            .get_block_by_height(height)
            .await?
            .ok_or_else(|| {
                ServiceError::invalid_state(format!(
                    "canonical block {height} is unavailable during header recovery"
                ))
            })?;
        hash_block(&block)
    }
}

fn hash_block(block: &Block) -> ServiceResult<UInt256> {
    block.try_hash().map_err(|error| {
        ServiceError::invalid_input(format!("hash block {}: {error}", block.index()))
    })
}

#[cfg(test)]
#[path = "../../tests/composition/sync_header_pipeline.rs"]
mod tests;
