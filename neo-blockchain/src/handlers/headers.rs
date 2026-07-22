use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use neo_payloads::header::Header;

use crate::command::HeaderValidationOutcome;
use crate::ledger_provider::{BlockProvider, ChainTipProvider};
use crate::pipeline::consensus_witness_stage::ParentHeaderContext;
use crate::pipeline::signature_verification::{
    HeaderSignaturePreverification, HeaderSignaturePreverificationTicket,
    SignatureVerificationCancellation, SignatureVerificationPool, SignatureVerificationSubmitError,
};
use crate::service::{BlockchainService, MempoolLike};

enum HeaderTicketDrain {
    Empty,
    Verified(Header),
    Rejected,
}

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Handle a [`BlockchainCommand::ValidateHeaders`] batch.
    ///
    /// C# `Blockchain.OnNewHeaders`: each header must chain onto the previous
    /// one and verify (`Header.Verify(settings, snapshot, headerCache)`) before
    /// it is cached; verification failure stops the batch (the C# `break`),
    /// keeping the valid prefix. The anchor for the first header is the last
    /// cached header, or the ledger tip when the cache is empty.
    pub(crate) fn handle_headers(&self, headers: Vec<Header>) -> HeaderValidationOutcome {
        let snapshot = self.system.store_snapshot();
        let settings = self.system.settings();
        let native_contract_provider = self.system.native_contract_provider();

        // C# verification anchor: HeaderCache.Last, else the ledger tip block.
        let mut prev: Option<Header> = self.header_cache.last();
        if prev.is_none()
            && let Some(snap) = &snapshot
        {
            let provider = self.system.ledger_provider(snap.as_ref());
            if let Ok(tip_hash) = provider.current_hash() {
                prev = provider.header_by_hash(&tip_hash).ok().flatten();
            }
        }

        let mut header_height = prev
            .as_ref()
            .map(|h| h.index())
            .unwrap_or_else(|| self.ledger.current_height());

        // Header validation has no canonical side effects until a header is
        // inserted into `HeaderCache`. When explicitly enabled, verify a
        // bounded window of exact signature operations in parallel. Every
        // header still crosses the ordered canonical NeoVM fence before it is
        // cached. A missing snapshot/anchor keeps the synchronous path below.
        if let (Some(pool), Some(snap), Some(provider), Some(anchor)) = (
            self.optimistic_signature_verification.as_ref(),
            snapshot.as_ref(),
            native_contract_provider.as_ref(),
            prev.as_ref(),
        ) {
            let verification_start = Instant::now();
            let metrics_before = pool.metrics_snapshot();
            let outcome = self.handle_headers_with_optimistic_pool(
                headers,
                Arc::clone(pool),
                Arc::clone(&settings),
                Arc::clone(snap),
                Arc::clone(provider),
                anchor.clone(),
                header_height,
            );
            let elapsed = verification_start.elapsed();
            if outcome.accepted > 0 {
                let blocks_per_second = outcome.accepted as f64 / elapsed.as_secs_f64().max(1e-9);
                let metrics_after = pool.metrics_snapshot();
                tracing::info!(
                    target: "neo::performance",
                    mode = "optimistic_signature_header",
                    blocks = outcome.accepted,
                    elapsed_ms = elapsed.as_secs_f64() * 1_000.0,
                    blocks_per_second,
                    signature_submitted = metrics_after.submitted.saturating_sub(metrics_before.submitted),
                    signature_completed = metrics_after.completed.saturating_sub(metrics_before.completed),
                    signature_invalid = metrics_after.invalid.saturating_sub(metrics_before.invalid),
                    signature_cancelled = metrics_after.cancelled.saturating_sub(metrics_before.cancelled),
                    signature_worker_panics = metrics_after.worker_panics.saturating_sub(metrics_before.worker_panics),
                    signature_worker_unavailable = metrics_after.worker_unavailable.saturating_sub(metrics_before.worker_unavailable),
                    signature_queue_full = metrics_after.queue_full.saturating_sub(metrics_before.queue_full),
                    signature_queue_closed = metrics_after.queue_closed.saturating_sub(metrics_before.queue_closed),
                    header_standard_caches_prepared = metrics_after.header_standard_caches_prepared.saturating_sub(metrics_before.header_standard_caches_prepared),
                    header_unsupported_witness_fallbacks = metrics_after.header_unsupported_witness_fallbacks.saturating_sub(metrics_before.header_unsupported_witness_fallbacks),
                    header_preverified_ecdsa_operations = metrics_after.header_preverified_ecdsa_operations.saturating_sub(metrics_before.header_preverified_ecdsa_operations),
                    header_canonical_cache_consumptions = metrics_after.header_canonical_cache_consumptions.saturating_sub(metrics_before.header_canonical_cache_consumptions),
                    header_canonical_cache_lookups = metrics_after.header_canonical_cache_lookups.saturating_sub(metrics_before.header_canonical_cache_lookups),
                    header_canonical_cache_hits = metrics_after.header_canonical_cache_hits.saturating_sub(metrics_before.header_canonical_cache_hits),
                    header_canonical_cache_misses = metrics_after.header_canonical_cache_misses.saturating_sub(metrics_before.header_canonical_cache_misses),
                    "optimistic header verification batch completed"
                );
            }
            return outcome;
        }

        let mut accepted = 0usize;

        for header in headers.into_iter() {
            let index = header.index();
            if index <= header_height {
                let known_hash = self.header_cache.hash_at(index).or_else(|| {
                    snapshot.as_ref().and_then(|snap| {
                        self.system
                            .ledger_provider(snap.as_ref())
                            .block_hash_by_index(index)
                            .ok()
                            .flatten()
                    })
                });
                match known_hash {
                    Some(hash) if hash == header.hash() => {
                        accepted += 1;
                        continue;
                    }
                    Some(_) => break,
                    None => continue,
                }
            }

            if index != header_height + 1 {
                break;
            }

            // C# Header.Verify(settings, snapshot, headerCache): primary index in
            // range, links onto the anchor, timestamp strictly increases, and the
            // consensus witness satisfies the anchor's NextConsensus (3-GAS cap).
            // Skipped only when no store snapshot is available (no anchor to
            // verify against, e.g. header-only unit fixtures).
            if let (Some(snap), Some(prev_header)) = (&snapshot, &prev) {
                let Some(provider) = &native_contract_provider else {
                    break;
                };
                if i32::from(header.primary_index()) >= settings.validators_count {
                    break;
                }
                if header.prev_hash() != &prev_header.hash() {
                    break;
                }
                if header.timestamp() <= prev_header.timestamp() {
                    break;
                }
                let next_consensus = *prev_header.next_consensus();
                if neo_execution::Helper::verify_witness_with_native_provider(
                    &header,
                    settings.as_ref(),
                    snap,
                    &next_consensus,
                    &header.witness,
                    300_000_000,
                    Arc::clone(provider),
                )
                .is_err()
                {
                    break;
                }
            }

            if !self.header_cache.add(header.clone()) {
                break;
            }

            accepted += 1;
            header_height = index;
            prev = Some(header);
        }

        HeaderValidationOutcome::new(accepted, prev)
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_headers_with_optimistic_pool(
        &self,
        headers: Vec<Header>,
        pool: Arc<SignatureVerificationPool>,
        settings: Arc<neo_config::ProtocolSettings>,
        snapshot: Arc<neo_storage::DataCache<S::CacheBacking>>,
        native_contract_provider: Arc<S::NativeProvider>,
        anchor: Header,
        anchor_height: u32,
    ) -> HeaderValidationOutcome {
        let mut pending: VecDeque<(
            Header,
            ParentHeaderContext,
            HeaderSignaturePreverificationTicket,
        )> = VecDeque::new();
        let cancellation = SignatureVerificationCancellation::default();
        let mut virtual_prev = anchor.clone();
        let mut virtual_height = anchor_height;
        let mut frontier = Some(anchor);
        let mut accepted = 0usize;
        let mut pool_enabled = true;

        for header in headers {
            let index = header.index();
            if index <= virtual_height {
                let known_hash = self.header_cache.hash_at(index).or_else(|| {
                    self.system
                        .ledger_provider(snapshot.as_ref())
                        .block_hash_by_index(index)
                        .ok()
                        .flatten()
                });
                match known_hash {
                    Some(hash) if hash == header.hash() => {
                        accepted += 1;
                        continue;
                    }
                    Some(_) => break,
                    None => continue,
                }
            }
            if index != virtual_height.saturating_add(1) {
                break;
            }

            // These are the same cheap checks performed before canonical
            // witness verification in the synchronous header path.
            if i32::from(header.primary_index()) >= settings.validators_count
                || header.prev_hash() != &virtual_prev.hash()
                || header.timestamp() <= virtual_prev.timestamp()
            {
                break;
            }
            let parent = ParentHeaderContext {
                hash: virtual_prev.hash(),
                index: virtual_prev.index(),
                timestamp: virtual_prev.timestamp(),
                next_consensus: *virtual_prev.next_consensus(),
            };

            if pool_enabled {
                while pending.len() >= pool.window() {
                    let HeaderTicketDrain::Verified(verified) = self
                        .drain_header_preverification_ticket(
                            &mut pending,
                            &cancellation,
                            settings.as_ref(),
                            snapshot.as_ref(),
                            Arc::clone(&native_contract_provider),
                        )
                    else {
                        return HeaderValidationOutcome::new(accepted, frontier);
                    };
                    if !self.header_cache.add(verified.clone()) {
                        return HeaderValidationOutcome::new(accepted, frontier);
                    }
                    accepted += 1;
                    frontier = Some(verified);
                }

                match pool.try_submit_header_witness_cancellable(
                    header.clone(),
                    Arc::clone(&settings),
                    &cancellation,
                ) {
                    Ok(ticket) => {
                        pending.push_back((header.clone(), parent, ticket));
                        virtual_prev = header;
                        virtual_height = index;
                        continue;
                    }
                    Err(SignatureVerificationSubmitError::QueueFull) => {
                        // The caller-side window normally prevents this. If a
                        // different consumer has filled the shared pool while
                        // this batch has no pending ticket, disable speculation
                        // and continue through the canonical synchronous path;
                        // queue contention must not truncate a valid header
                        // batch. With pending work, drain one ticket and retry
                        // once so an invalid older prefix still stops the batch.
                        if pending.is_empty() {
                            pool_enabled = false;
                        } else {
                            let HeaderTicketDrain::Verified(verified) = self
                                .drain_header_preverification_ticket(
                                    &mut pending,
                                    &cancellation,
                                    settings.as_ref(),
                                    snapshot.as_ref(),
                                    Arc::clone(&native_contract_provider),
                                )
                            else {
                                return HeaderValidationOutcome::new(accepted, frontier);
                            };
                            if !self.header_cache.add(verified.clone()) {
                                return HeaderValidationOutcome::new(accepted, frontier);
                            }
                            accepted += 1;
                            frontier = Some(verified);
                            match pool.try_submit_header_witness_cancellable(
                                header.clone(),
                                Arc::clone(&settings),
                                &cancellation,
                            ) {
                                Ok(ticket) => {
                                    pending.push_back((header.clone(), parent, ticket));
                                    virtual_prev = header;
                                    virtual_height = index;
                                    continue;
                                }
                                Err(_) => pool_enabled = false,
                            }
                        }
                    }
                    Err(SignatureVerificationSubmitError::Closed)
                    | Err(SignatureVerificationSubmitError::InvalidInput(_)) => {
                        pool_enabled = false;
                    }
                }
            }

            // Pool shutdown/preparation failures fall back to the canonical
            // synchronous verifier. First drain older speculative work so the
            // parent context is an already accepted frontier.
            loop {
                match self.drain_header_preverification_ticket(
                    &mut pending,
                    &cancellation,
                    settings.as_ref(),
                    snapshot.as_ref(),
                    Arc::clone(&native_contract_provider),
                ) {
                    HeaderTicketDrain::Empty => break,
                    HeaderTicketDrain::Rejected => {
                        return HeaderValidationOutcome::new(accepted, frontier);
                    }
                    HeaderTicketDrain::Verified(verified) => {
                        if !self.header_cache.add(verified.clone()) {
                            return HeaderValidationOutcome::new(accepted, frontier);
                        }
                        accepted += 1;
                        frontier = Some(verified);
                    }
                }
            }
            let Some(actual_parent) = frontier.as_ref() else {
                break;
            };
            let parent = ParentHeaderContext {
                hash: actual_parent.hash(),
                index: actual_parent.index(),
                timestamp: actual_parent.timestamp(),
                next_consensus: *actual_parent.next_consensus(),
            };
            if !self.verify_header_witness_with_preverification(
                &header,
                &parent,
                settings.as_ref(),
                snapshot.as_ref(),
                Arc::clone(&native_contract_provider),
                None,
            ) {
                break;
            }
            if !self.header_cache.add(header.clone()) {
                break;
            }
            accepted += 1;
            virtual_prev = header.clone();
            virtual_height = index;
            frontier = Some(header);
        }

        // Ordered publication fence for all remaining speculative headers.
        loop {
            match self.drain_header_preverification_ticket(
                &mut pending,
                &cancellation,
                settings.as_ref(),
                snapshot.as_ref(),
                Arc::clone(&native_contract_provider),
            ) {
                HeaderTicketDrain::Empty => break,
                HeaderTicketDrain::Rejected => {
                    return HeaderValidationOutcome::new(accepted, frontier);
                }
                HeaderTicketDrain::Verified(verified) => {
                    if !self.header_cache.add(verified.clone()) {
                        break;
                    }
                    accepted += 1;
                    frontier = Some(verified);
                }
            }
        }

        HeaderValidationOutcome::new(accepted, frontier)
    }

    fn drain_header_preverification_ticket(
        &self,
        pending: &mut VecDeque<(
            Header,
            ParentHeaderContext,
            HeaderSignaturePreverificationTicket,
        )>,
        cancellation: &SignatureVerificationCancellation,
        settings: &neo_config::ProtocolSettings,
        snapshot: &neo_storage::DataCache<S::CacheBacking>,
        native_contract_provider: Arc<S::NativeProvider>,
    ) -> HeaderTicketDrain {
        let Some((header, parent, ticket)) = pending.pop_front() else {
            return HeaderTicketDrain::Empty;
        };
        let preverification = ticket.wait().ok().flatten();
        if self.verify_header_witness_with_preverification(
            &header,
            &parent,
            settings,
            snapshot,
            native_contract_provider,
            preverification.as_ref(),
        ) {
            HeaderTicketDrain::Verified(header)
        } else {
            cancellation.cancel();
            pending.clear();
            HeaderTicketDrain::Rejected
        }
    }

    fn verify_header_witness_with_preverification(
        &self,
        header: &Header,
        parent: &ParentHeaderContext,
        settings: &neo_config::ProtocolSettings,
        snapshot: &neo_storage::DataCache<S::CacheBacking>,
        native_contract_provider: Arc<S::NativeProvider>,
        preverification: Option<&HeaderSignaturePreverification>,
    ) -> bool {
        if parent.index.checked_add(1) != Some(header.index())
            || header.prev_hash() != &parent.hash
            || header.timestamp() <= parent.timestamp
            || i32::from(header.primary_index()) >= settings.validators_count
        {
            return false;
        }

        let signature_cache = preverification
            .filter(|proof| proof.matches(header, settings))
            .map(HeaderSignaturePreverification::signature_cache);
        match signature_cache {
            Some(signature_cache) => {
                let cache_metrics_before = signature_cache.metrics_snapshot();
                let result =
                    neo_execution::Helper::verify_witness_with_native_provider_and_signature_cache(
                        header,
                        settings,
                        snapshot,
                        &parent.next_consensus,
                        &header.witness,
                        300_000_000,
                        native_contract_provider,
                        Arc::clone(&signature_cache),
                    );
                self.record_header_signature_cache_consumption(
                    signature_cache.as_ref(),
                    cache_metrics_before,
                );
                result.is_ok()
            }
            None => neo_execution::Helper::verify_witness_with_native_provider(
                header,
                settings,
                snapshot,
                &parent.next_consensus,
                &header.witness,
                300_000_000,
                native_contract_provider,
            )
            .is_ok(),
        }
    }
}
