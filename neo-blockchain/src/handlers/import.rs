use std::sync::Arc;
use std::time::Instant;

use neo_config::ProtocolSettings;
use neo_payloads::Block;
use tracing::warn;

use crate::block_processing::BlockCommitArtifacts;
use crate::command::{ImportBlocksReply, ImportBlocksStats};
use crate::import::Import;
use crate::internal::ImportDisposition;
use crate::pipeline::signature_verification::{
    SignatureVerificationError, TransactionSignatureVerificationTicket,
};
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
        let mut deferred_committed_blocks = Vec::new();
        let mut import_error = None;
        let mut batch_persist_resources = None;
        let mut batch_persist_resources_loaded = false;
        let mut last_imported_height = None;
        let mut optimistic_signature_jobs = 0usize;
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
            let observer_requires_artifacts = plan.allows_replay_artifacts()
                && self
                    .system
                    .requires_replay_artifacts(block, persist_context);
            let persist_options = plan.persist_options(observer_requires_artifacts);

            // In a deferred sync batch, standard transaction signatures can be
            // checked on dedicated workers while this block executes against
            // the staged cache. The commit fence below waits for every ticket;
            // a failed ticket aborts the staged store and rewinds the in-memory
            // ledger to the last durable checkpoint.
            let pending_transaction_signatures = if verify && defer_store_commit {
                match self.submit_optimistic_transaction_signatures(
                    block,
                    self.system.chain_spec().protocol_settings_arc(),
                ) {
                    Ok((tickets, submitted)) => {
                        optimistic_signature_jobs += submitted;
                        tickets
                    }
                    Err(error) => {
                        warn!(
                            target: "neo",
                            height = index,
                            %error,
                            "optimistic transaction signature preflight failed"
                        );
                        import_error = Some(error);
                        break;
                    }
                }
            } else {
                Vec::new()
            };

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
                )
            {
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

            if let Err(error) = self.wait_optimistic_transaction_signatures(
                block,
                pending_transaction_signatures,
                self.system.chain_spec().protocol_settings(),
            ) {
                let error = format!(
                    "import aborted at height {index}: optimistic transaction signature verification failed: {error}"
                );
                self.system.abort_store_commit();
                self.ledger.rewind_to(durable_checkpoint_height);
                return ImportBlocksReply::failed_with_stats(
                    already_durable
                        + durable_checkpoint_height.saturating_sub(durable_height) as usize,
                    stats,
                    error,
                );
            }
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
            return ImportBlocksReply::failed_with_stats(imported, stats, error);
        }
        if verify && defer_store_commit && imported > 0 && optimistic_signature_jobs > 0 {
            let elapsed = import_start.elapsed();
            tracing::info!(
                target: "neo::performance",
                mode = "optimistic_signature_import",
                blocks = imported,
                signature_jobs = optimistic_signature_jobs,
                elapsed_ms = elapsed.as_secs_f64() * 1_000.0,
                blocks_per_second = imported as f64 / elapsed.as_secs_f64().max(1e-9),
                "optimistic transaction signature import completed"
            );
        }
        ImportBlocksReply::ok_with_stats(imported, stats)
    }

    fn submit_optimistic_transaction_signatures(
        &self,
        block: &Block,
        settings: Arc<ProtocolSettings>,
    ) -> Result<(Vec<(usize, TransactionSignatureVerificationTicket)>, usize), String> {
        let Some(pool) = self.optimistic_signature_verification.as_ref() else {
            return Ok((Vec::new(), 0));
        };

        let mut tickets = Vec::new();
        let mut submitted = 0usize;
        for (transaction_index, transaction) in block.transactions.iter().enumerate() {
            if !neo_mempool::transaction_witnesses_are_state_independent(transaction) {
                continue;
            }

            let fallback = || {
                let result = neo_mempool::verify_state_independent(transaction, settings.as_ref());
                (result == neo_primitives::VerifyResult::Succeed).then_some(())
            };
            match pool.try_submit_transaction_state_independent(
                Arc::new(transaction.clone()),
                Arc::clone(&settings),
            ) {
                Ok(ticket) => {
                    submitted += 1;
                    tickets.push((transaction_index, ticket));
                }
                Err(crate::pipeline::signature_verification::SignatureVerificationSubmitError::QueueFull)
                | Err(crate::pipeline::signature_verification::SignatureVerificationSubmitError::Closed) => {
                    if fallback().is_none() {
                        return Err(format!(
                            "transaction {transaction_index} ({}) failed state-independent verification",
                            transaction.try_hash().map_or_else(|_| "hash-error".to_owned(), |hash| hash.to_string())
                        ));
                    }
                }
                Err(crate::pipeline::signature_verification::SignatureVerificationSubmitError::InvalidInput(reason)) => {
                    return Err(format!(
                        "transaction {transaction_index} cannot enter optimistic signature lane: {reason}"
                    ));
                }
            }
        }
        Ok((tickets, submitted))
    }

    fn wait_optimistic_transaction_signatures(
        &self,
        block: &Block,
        tickets: Vec<(usize, TransactionSignatureVerificationTicket)>,
        settings: &ProtocolSettings,
    ) -> Result<(), String> {
        for (transaction_index, ticket) in tickets {
            let transaction = block
                .transactions
                .get(transaction_index)
                .ok_or_else(|| format!("transaction {transaction_index} disappeared"))?;
            match ticket.wait() {
                Ok(receipt) if receipt.matches(transaction, settings) => {}
                Ok(_)
                | Err(
                    SignatureVerificationError::WorkerPanicked
                    | SignatureVerificationError::WorkerUnavailable,
                ) => {
                    let result = neo_mempool::verify_state_independent(transaction, settings);
                    if result != neo_primitives::VerifyResult::Succeed {
                        return Err(format!(
                            "transaction {transaction_index} failed synchronous fallback: {result:?}"
                        ));
                    }
                }
                Err(SignatureVerificationError::InvalidWitness(reason)) => {
                    return Err(format!("transaction {transaction_index}: {reason}"));
                }
            }
        }
        Ok(())
    }
}
