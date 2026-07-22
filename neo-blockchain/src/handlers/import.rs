use std::sync::Arc;
use std::time::Instant;
use tracing::{info, warn};

use crate::block_processing::BlockCommitArtifacts;
use crate::command::{ImportBlocksReply, ImportBlocksStats};
use crate::import::Import;
use crate::internal::ImportDisposition;
use crate::pipeline::signature_verification::OrderedHeaderVerificationWindow;
use crate::service::{BlockchainService, MempoolLike};

mod empty_fast_forward;
mod finalization;
mod persist;
mod plan;
mod verification;

use plan::ImportPlan;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Handle a [`BlockchainCommand::Import`] request.
    pub(crate) async fn handle_import(&self, import: Import) -> ImportBlocksReply {
        let import_start = Instant::now();
        let mut imported = 0usize;
        let mut already_durable = 0usize;
        let mut stats = ImportBlocksStats::default();
        let blocks = import.blocks;
        let durable_height = self.ledger.current_height();
        let plan = ImportPlan::resolve(import.mode, &blocks, durable_height, self.system.as_ref());
        let verify = plan.verify();
        let trusted_replay = plan.is_trusted_replay();
        let persist_context = plan.persist_context();
        let defer_store_commit = plan.defers_store_commit();
        let settings = self.system.chain_spec().protocol_settings_arc();
        let signature_metrics_before = self
            .optimistic_signature_verification
            .as_ref()
            .map(|pool| pool.metrics_snapshot());
        let mut signature_window = OrderedHeaderVerificationWindow::default();
        let mut deferred_committed_blocks = Vec::new();
        let mut import_error = None;
        let mut batch_persist_resources = None;
        let mut batch_persist_resources_loaded = false;
        let mut last_imported_height = None;
        // Last height that was already intermediate-committed durably.
        let mut durable_checkpoint_height = durable_height;
        let mut position = 0usize;
        while position < blocks.len() {
            let block = &blocks[position];
            let index = block.index();
            let current_height = self.ledger.current_height();
            match ImportDisposition::classify_import_block(current_height, index) {
                ImportDisposition::AlreadySeen => {
                    imported += 1;
                    already_durable += 1;
                    position += 1;
                    continue;
                }
                ImportDisposition::FutureGap => {
                    warn!(
                        target: "neo",
                        expected = current_height + 1,
                        actual = index,
                        "import block out of sequence"
                    );
                    break;
                }
                ImportDisposition::NextExpected => {}
            }
            let current_signature = verify
                .then(|| signature_window.take_current(block))
                .flatten();
            let observer_requires_artifacts = plan.allows_replay_artifacts()
                && self
                    .system
                    .requires_replay_artifacts(block, persist_context);
            let persist_options = plan.persist_options(observer_requires_artifacts);

            if defer_store_commit && !batch_persist_resources_loaded {
                match self.batch_persist_resources(index) {
                    Ok(resources) => {
                        batch_persist_resources = resources;
                    }
                    Err(error) => {
                        warn!(
                            target: "neo",
                            %error,
                            height = index,
                            "import aborted: native persistence resource setup failed"
                        );
                        import_error = Some(error.to_string());
                        break;
                    }
                }
                batch_persist_resources_loaded = true;
            }

            if verify && let Some(pool) = self.optimistic_signature_verification.as_ref() {
                signature_window.fill_after(position, &blocks, pool, Arc::clone(&settings));
            }
            let signature_preverification = current_signature
                .and_then(|ticket| ticket.wait().ok())
                .flatten();

            if trusted_replay
                && !verify
                && let Some(resources) = &batch_persist_resources
                && let Some((fast_forwarded, last_height)) = self.try_bulk_empty_fast_forward(
                    &blocks,
                    position,
                    current_height,
                    resources,
                    &mut stats,
                )
            {
                imported += fast_forwarded;
                last_imported_height = Some(last_height);
                // The fast-forward helper stages the state-equivalent writes
                // directly into the shared snapshot. Retain the accepted
                // block identities so the deferred finalization path still
                // performs one durable commit and publishes the same ordered
                // block boundary as normal persistence.
                deferred_committed_blocks.extend(
                    blocks[position..position + fast_forwarded]
                        .iter()
                        .cloned()
                        .map(|block| (std::sync::Arc::new(block), false)),
                );
                position += fast_forwarded;
                continue;
            }

            if verify
                && !self.verify_import_block_for_command(
                    block,
                    current_height,
                    trusted_replay,
                    batch_persist_resources.as_ref(),
                    signature_preverification.as_ref(),
                )
            {
                signature_window.disable();
                import_error = Some(format!(
                    "block {} failed canonical import verification",
                    block.index()
                ));
                break;
            }

            if trusted_replay && let Some(resources) = &batch_persist_resources {
                let empty_start = Instant::now();
                match self.persist_empty_block_with_committing_fast_forward(
                    block,
                    current_height,
                    resources,
                    persist_options,
                    persist_context,
                ) {
                    Ok(true) => {
                        stats.empty_blocks += 1;
                        stats.empty_elapsed += empty_start.elapsed();
                        imported += 1;
                        last_imported_height = Some(index);
                        deferred_committed_blocks.push((std::sync::Arc::new(block.clone()), true));
                        position += 1;
                        continue;
                    }
                    Ok(false) => {}
                    Err(error) => {
                        warn!(
                            target: "neo",
                            height = index,
                            %error,
                            "import aborted: empty-block committing fast-forward failed"
                        );
                        import_error = Some(error.to_string());
                        break;
                    }
                }
            }

            let committed_block = match self
                .persist_import_block_for_command(
                    &blocks[position],
                    defer_store_commit,
                    persist_options,
                    persist_context,
                    batch_persist_resources.as_ref(),
                    &mut stats,
                )
                .await
            {
                Ok(block) => block,
                Err(error) => {
                    if self.ledger.current_height() >= index {
                        imported += 1;
                        last_imported_height = Some(index);
                    }
                    warn!(target: "neo", %error, height = index, "block import persistence failed");
                    import_error = Some(error);
                    break;
                }
            };

            imported += 1;
            last_imported_height = Some(index);
            if defer_store_commit {
                deferred_committed_blocks.push((committed_block, true));
                // Intermediate Ledger+StateService co-commit when the projected
                // MPT change budget is reached (coordinated catch-up path).
                if let Some(budget) = self.system.deferred_import_work_budget() {
                    if !deferred_committed_blocks.is_empty()
                        && self.system.pending_deferred_import_work() >= budget
                    {
                        let staged = deferred_committed_blocks.len();
                        if let Err(error) = self.finalize_deferred_import(staged, &mut stats) {
                            self.system.abort_store_commit();
                            self.ledger.rewind_to(durable_checkpoint_height);
                            return ImportBlocksReply::failed_with_stats(
                                already_durable
                                    + durable_checkpoint_height.saturating_sub(durable_height)
                                        as usize,
                                stats,
                                error,
                            );
                        }
                        for (block, publish_finalized) in deferred_committed_blocks.drain(..) {
                            if publish_finalized {
                                let finalized_delivery_start =
                                    (!block.transactions.is_empty()).then(std::time::Instant::now);
                                let finalized =
                                    BlockCommitArtifacts::without_replay_artifacts(None)
                                        .into_finalized(
                                            std::sync::Arc::clone(&block),
                                            persist_context,
                                        );
                                if let Err(error) = self.system.block_finalized(finalized).await {
                                    import_error = Some(format!(
                                        "block {} committed durably but finalized delivery failed: {error}",
                                        block.index()
                                    ));
                                    break;
                                }
                                if plan.maintains_live_side_effects() {
                                    self.mempool.block_persisted(block.as_ref());
                                    if let Ok(hash) = Self::try_block_hash(block.as_ref()) {
                                        self.event_tx
                                            .send(crate::RuntimeEvent::Imported {
                                                hash,
                                                height: block.index(),
                                                timestamp: block.timestamp(),
                                            })
                                            .ok();
                                    }
                                }
                                if let Some(start) = finalized_delivery_start {
                                    stats.transaction_finalized_delivery_elapsed += start.elapsed();
                                }
                            }
                            durable_checkpoint_height = block.index();
                        }
                        if import_error.is_some() {
                            break;
                        }
                    }
                }
            }
            position += 1;
            if self.system.should_stop_blockchain_service() {
                import_error.get_or_insert_with(|| {
                    format!(
                        "import stopped after {} block {index}: canonical writer shutdown requested",
                        if defer_store_commit {
                            "staged"
                        } else {
                            "durable"
                        }
                    )
                });
                break;
            }
        }
        if defer_store_commit {
            if self.system.should_stop_blockchain_service() {
                self.system.abort_store_commit();
                self.ledger.rewind_to(durable_checkpoint_height);
                return ImportBlocksReply::failed_with_stats(
                    already_durable
                        + durable_checkpoint_height.saturating_sub(durable_height) as usize,
                    stats,
                    import_error.unwrap_or_else(|| {
                        "deferred import aborted after a fatal persistence failure".to_string()
                    }),
                );
            }
            let newly_staged = deferred_committed_blocks.len();
            if let Err(error) = self.finalize_deferred_import(newly_staged, &mut stats) {
                self.ledger.rewind_to(durable_checkpoint_height);
                return ImportBlocksReply::failed_with_stats(
                    already_durable
                        + durable_checkpoint_height.saturating_sub(durable_height) as usize,
                    stats,
                    error,
                );
            }
            for (block, publish_finalized) in deferred_committed_blocks {
                if publish_finalized {
                    let finalized_delivery_start =
                        (!block.transactions.is_empty()).then(std::time::Instant::now);
                    let finalized = BlockCommitArtifacts::without_replay_artifacts(None)
                        .into_finalized(std::sync::Arc::clone(&block), persist_context);
                    if let Err(error) = self.system.block_finalized(finalized).await {
                        import_error = Some(format!(
                            "block {} committed durably but finalized delivery failed: {error}",
                            block.index()
                        ));
                        break;
                    }
                    if plan.maintains_live_side_effects() {
                        self.mempool.block_persisted(block.as_ref());
                        if let Ok(hash) = Self::try_block_hash(block.as_ref()) {
                            self.event_tx
                                .send(crate::RuntimeEvent::Imported {
                                    hash,
                                    height: block.index(),
                                    timestamp: block.timestamp(),
                                })
                                .ok();
                        }
                    }
                    if let Some(start) = finalized_delivery_start {
                        stats.transaction_finalized_delivery_elapsed += start.elapsed();
                    }
                }
            }
            self.finish_deferred_import_cache_maintenance(
                last_imported_height,
                plan.maintains_live_side_effects(),
            )
            .await;
            if self.system.should_stop_blockchain_service() {
                import_error.get_or_insert_with(|| {
                    "deferred import committed durably but canonical writer shutdown was requested"
                        .to_string()
                });
            }
        }
        if let Some(error) = import_error {
            self.log_optimistic_signature_import(
                import_start,
                imported.saturating_sub(already_durable),
                verify,
                signature_metrics_before,
                &signature_window,
            );
            return ImportBlocksReply::failed_with_stats(imported, stats, error);
        }
        self.log_optimistic_signature_import(
            import_start,
            imported.saturating_sub(already_durable),
            verify,
            signature_metrics_before,
            &signature_window,
        );
        ImportBlocksReply::ok_with_stats(imported, stats)
    }

    fn log_optimistic_signature_import(
        &self,
        started: Instant,
        imported: usize,
        verify: bool,
        metrics_before: Option<
            crate::pipeline::signature_verification::SignatureVerificationPoolMetricsSnapshot,
        >,
        window: &OrderedHeaderVerificationWindow,
    ) {
        if !verify || imported == 0 {
            return;
        }
        let Some(pool) = self.optimistic_signature_verification.as_ref() else {
            return;
        };
        let before = metrics_before.unwrap_or_default();
        let after = pool.metrics_snapshot();
        let elapsed = started.elapsed();
        info!(
            target: "neo::performance",
            mode = "optimistic_signature_verified_import",
            blocks = imported,
            elapsed_ms = elapsed.as_secs_f64() * 1_000.0,
            blocks_per_second = imported as f64 / elapsed.as_secs_f64().max(1e-9),
            signature_submitted = after.submitted.saturating_sub(before.submitted),
            signature_completed = after.completed.saturating_sub(before.completed),
            signature_cancelled = after.cancelled.saturating_sub(before.cancelled),
            signature_worker_panics = after.worker_panics.saturating_sub(before.worker_panics),
            signature_worker_unavailable = after
                .worker_unavailable
                .saturating_sub(before.worker_unavailable),
            signature_queue_full = after.queue_full.saturating_sub(before.queue_full),
            signature_queue_closed = after.queue_closed.saturating_sub(before.queue_closed),
            header_standard_caches_prepared = after
                .header_standard_caches_prepared
                .saturating_sub(before.header_standard_caches_prepared),
            header_unsupported_witness_fallbacks = after
                .header_unsupported_witness_fallbacks
                .saturating_sub(before.header_unsupported_witness_fallbacks),
            header_preverified_ecdsa_operations = after
                .header_preverified_ecdsa_operations
                .saturating_sub(before.header_preverified_ecdsa_operations),
            header_canonical_cache_consumptions = after
                .header_canonical_cache_consumptions
                .saturating_sub(before.header_canonical_cache_consumptions),
            header_canonical_cache_lookups = after
                .header_canonical_cache_lookups
                .saturating_sub(before.header_canonical_cache_lookups),
            header_canonical_cache_hits = after
                .header_canonical_cache_hits
                .saturating_sub(before.header_canonical_cache_hits),
            header_canonical_cache_misses = after
                .header_canonical_cache_misses
                .saturating_sub(before.header_canonical_cache_misses),
            signature_prefetched_headers = window.submitted(),
            signature_max_pending = window.max_pending(),
            "optimistic verified import completed"
        );
    }
}
