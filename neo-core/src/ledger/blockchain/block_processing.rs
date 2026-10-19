//
// block_processing.rs - Block processing logic for Blockchain actor
//

use super::*;
use dashmap::mapref::entry::Entry;
use std::sync::atomic::Ordering;
use tracing::instrument;

/// Maximum blocks to persist in a single drain pass.
/// After this many, the actor yields to process queued messages and
/// self-schedules another DrainUnverified to continue the sequence.
const DRAIN_BATCH_SIZE: usize = 50;

impl Blockchain {
    #[instrument(name = "blockchain_process_block", skip(self, block), fields(height = block.index(), hash = tracing::field::Empty))]
    pub(super) async fn on_new_block(&self, block: Arc<Block>, verify: bool) -> VerifyResult {
        let span = tracing::Span::current();
        if let Ok(hash) = Self::try_block_hash(block.as_ref()) {
            span.record("hash", tracing::field::display(&hash));
        }
        let Some(context) = &self.system_context else {
            return VerifyResult::Invalid;
        };

        let block_index = block.index();
        let hash = match Self::try_block_hash(block.as_ref()) {
            Ok(hash) => hash,
            Err(error) => {
                tracing::warn!(
                    target: "neo",
                    error = %error,
                    index = block_index,
                    "block hash computation failed, rejecting block"
                );
                return VerifyResult::Invalid;
            }
        };

        let store_cache = context.store_cache();
        let settings = context.settings();
        let header_cache = context.header_cache();

        let current_height = context.ledger().current_height();
        let header_height = header_cache
            .last()
            .map(|header| header.index())
            .unwrap_or(current_height);

        if block_index.is_multiple_of(10000) {
            tracing::info!(
                target: "neo",
                block_index,
                current_height,
                header_height,
                "on_new_block milestone"
            );
        }

        if block_index <= current_height {
            return VerifyResult::AlreadyExists;
        }

        if block_index > header_height + 1 {
            self.add_unverified_block(Arc::clone(&block)).await;
            return VerifyResult::UnableToVerify;
        }

        if verify {
            if block_index == header_height + 1 {
                if !block.verify_with_cache(settings.as_ref(), &store_cache, &header_cache) {
                    tracing::warn!(
                        target: "neo",
                        index = block_index,
                        %hash,
                        prev = %block.prev_hash(),
                        "block verification failed against header cache"
                    );
                    return VerifyResult::Invalid;
                }
            } else {
                let Some(mut header) = header_cache.get(block_index) else {
                    return VerifyResult::Invalid;
                };

                let cached_header_hash = match header.try_hash() {
                    Ok(hash) => hash,
                    Err(error) => {
                        tracing::warn!(
                            target: "neo",
                            error = %error,
                            index = block_index,
                            "cached header hash computation failed"
                        );
                        return VerifyResult::Invalid;
                    }
                };

                if cached_header_hash != hash {
                    tracing::warn!(
                        target: "neo",
                        index = block_index,
                        %hash,
                        "block hash does not match cached header"
                    );
                    return VerifyResult::Invalid;
                }
            }
        }

        let block_cache_full =
            self._block_cache.len() >= MAX_BLOCK_CACHE_SIZE && block_index != current_height + 1;
        match self._block_cache.entry(hash) {
            Entry::Occupied(_) => return VerifyResult::AlreadyExists,
            Entry::Vacant(entry) if block_cache_full => {
                drop(entry);
                // Cache is full but this isn't the next block to persist.
                // Park it in the unverified cache so persist_block_sequence
                // can pick it up later, and return OutOfMemory (not Invalid)
                // so the task manager does not disconnect the peer.
                self.add_unverified_block(Arc::clone(&block)).await;
                return VerifyResult::OutOfMemory;
            }
            Entry::Vacant(entry) => {
                entry.insert(Arc::clone(&block));
            }
        }

        if block_index == current_height + 1 {
            if self.persist_block_sequence(Arc::clone(&block)).await {
                VerifyResult::Succeed
            } else {
                // Persistence failed — remove from block cache so the block
                // can be retried if it arrives again (e.g. from another peer).
                self._block_cache.remove(&hash);
                VerifyResult::Invalid
            }
        } else {
            if block_index == header_height + 1 {
                header_cache.add(block.header.clone());
            }
            self.add_unverified_block(Arc::clone(&block)).await;
            VerifyResult::Succeed
        }
    }

    pub(super) async fn add_unverified_block(&self, block: Arc<Block>) {
        let block_hash = match Self::try_block_hash(block.as_ref()) {
            Ok(hash) => hash,
            Err(error) => {
                warn!(
                    target: "neo",
                    error = %error,
                    index = block.index(),
                    "unverified block hash computation failed, dropping block"
                );
                return;
            }
        };
        let index = block.index();
        let cost = Self::unverified_block_cost(block.as_ref());

        if cost > MAX_UNVERIFIED_CACHE_BYTES {
            warn!(
                target: "neo",
                index,
                cost,
                limit = MAX_UNVERIFIED_CACHE_BYTES,
                "unverified block exceeds cache byte budget, dropping block"
            );
            return;
        }

        // Deduplicate by hash and bound the number of fork candidates kept for
        // a single height. Without these caps a peer can pin unbounded memory
        // by sending many different blocks for the same future height.
        //
        // When the per-height cap is full we evict the oldest candidate and
        // accept the new one. Silently dropping the newcomer let an attacker
        // pre-fill every future height with cheap bogus blocks and censor the
        // honest block (whose hash is then suppressed by the peer dedup cache).
        let mut evicted_cost = 0usize;
        if let Some(mut existing) = self._block_cache_unverified.get_mut(&index) {
            let duplicate = existing
                .blocks
                .iter()
                .any(|candidate| Self::try_block_hash(candidate.as_ref()).ok() == Some(block_hash));
            if duplicate {
                return;
            }
            if existing.blocks.len() >= MAX_UNVERIFIED_CANDIDATES_PER_HEIGHT
                && let Some(evicted) = existing.blocks.first().cloned()
            {
                existing.blocks.remove(0);
                evicted_cost = Self::unverified_block_cost(evicted.as_ref());
            }
        }
        if evicted_cost > 0 {
            let _ = self._unverified_cache_bytes.fetch_update(
                Ordering::Relaxed,
                Ordering::Relaxed,
                |current| Some(current.saturating_sub(evicted_cost)),
            );
            let _ = self._unverified_cache_blocks.fetch_update(
                Ordering::Relaxed,
                Ordering::Relaxed,
                |current| Some(current.saturating_sub(1)),
            );
        }

        if !self.evict_unverified_for(cost) {
            warn!(
                target: "neo",
                index,
                cost,
                "unable to free space in unverified block cache, dropping block"
            );
            return;
        }

        self._block_cache_unverified
            .entry(index)
            .or_insert_with(UnverifiedBlocksList::new)
            .blocks
            .push(block);
        self._unverified_cache_bytes
            .fetch_add(cost, Ordering::Relaxed);
        self._unverified_cache_blocks
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Evicts the highest-index unverified entries until `incoming_cost` fits
    /// within both the block-count and byte budgets. Returns false when no
    /// further eviction is possible and the incoming block still does not fit.
    fn evict_unverified_for(&self, incoming_cost: usize) -> bool {
        loop {
            let bytes = self._unverified_cache_bytes.load(Ordering::Relaxed);
            let blocks = self._unverified_cache_blocks.load(Ordering::Relaxed);
            if bytes.saturating_add(incoming_cost) <= MAX_UNVERIFIED_CACHE_BYTES
                && blocks < MAX_UNVERIFIED_CACHE_SIZE
            {
                return true;
            }

            let Some(max_index) = self
                ._block_cache_unverified
                .iter()
                .map(|entry| *entry.key())
                .max()
            else {
                return false;
            };

            if self.remove_unverified_index(max_index).is_none() {
                return false;
            }
        }
    }

    async fn persist_block_sequence(&self, block: Arc<Block>) -> bool {
        let Some(context) = self.system_context.as_ref() else {
            return false;
        };
        let mut next_index = block.index().saturating_add(1);
        let mut persisted_count = 1usize; // count the first block

        // Process the first block. It was verified by `on_new_block` immediately
        // before this call when it was the next height.
        let first_succeeded = self.persist_block_via_system(&block);
        if first_succeeded {
            self.handle_persist_completed(PersistCompleted { block })
                .await;
        } else {
            return false;
        }

        // Process subsequent blocks from the unverified cache
        loop {
            // Batch limit: yield control back to the actor so queued messages
            // (new InventoryBlock, headers, etc.) can be processed.
            // A DrainUnverified will continue the sequence.
            if persisted_count >= DRAIN_BATCH_SIZE {
                tracing::debug!(
                    target: "neo",
                    persisted_count,
                    next_index,
                    "persist_block_sequence: batch limit reached, yielding to actor"
                );
                break;
            }

            let maybe_block = self.pop_from_unverified(next_index);

            let Some(next_block) = maybe_block else {
                break;
            };

            // Blocks in the unverified cache were not validated when they
            // arrived. Validate against the verified header (or run full
            // verification) before committing state. Try any remaining fork
            // candidates at this height before giving up.
            if !self.verify_block_for_persistence(next_block.as_ref(), context) {
                warn!(
                    target: "neo",
                    index = next_block.index(),
                    "dropping unverified block that failed pre-persist verification"
                );
                continue;
            }

            let succeeded = self.persist_block_via_system(&next_block);
            if succeeded {
                self.handle_persist_completed(PersistCompleted {
                    block: Arc::clone(&next_block),
                })
                .await;
                persisted_count += 1;
            } else {
                warn!(
                    target: "neo",
                    index = next_block.index(),
                    "stopping contiguous persistence sequence after block persist failure"
                );
                break;
            }
            next_index = next_index.saturating_add(1);
        }
        true
    }

    /// Drain consecutive blocks from the unverified cache starting at current_height+1.
    /// Self-schedules another DrainUnverified if more blocks remain after the batch limit.
    /// This ensures persistence continues even when the specific InventoryBlock message
    /// for the next block is delayed behind stale messages in the actor mailbox.
    pub(super) async fn handle_drain_unverified(&self, ctx: &ActorContext) {
        let Some(context) = &self.system_context else {
            return;
        };

        let mut current_height = context.ledger().current_height();
        let mut persisted = 0usize;

        while persisted < DRAIN_BATCH_SIZE {
            let next_index = current_height.saturating_add(1);
            // Try unverified cache first, then fall back to block cache.
            // Blocks may land in block_cache but miss the unverified cache
            // if they arrived as current_height+1 but persistence was busy.
            let next_block = self
                .pop_from_unverified(next_index)
                .or_else(|| self.find_block_in_cache_by_index(context, next_index));
            let Some(block) = next_block else {
                break;
            };

            // The block may have come from the unverified cache (never
            // validated) or from the verified block cache. Re-check it against
            // the verified header chain before committing state.
            if !self.verify_block_for_persistence(block.as_ref(), context) {
                warn!(
                    target: "neo",
                    index = next_index,
                    "drain_unverified: block failed pre-persist verification"
                );
                continue;
            }

            // Insert into block_cache for dedup only after verification.
            let hash = match Self::try_block_hash(block.as_ref()) {
                Ok(hash) => hash,
                Err(error) => {
                    tracing::warn!(
                        target: "neo",
                        error = %error,
                        index = next_index,
                        "drain_unverified: block hash computation failed"
                    );
                    break;
                }
            };
            self._block_cache.insert(hash, Arc::clone(&block));

            let succeeded = self.persist_block_via_system(&block);
            if succeeded {
                self.handle_persist_completed(PersistCompleted {
                    block: Arc::clone(&block),
                })
                .await;
                current_height = next_index;
                persisted += 1;
            } else {
                // Allow the block to be retried when it arrives again.
                self._block_cache.remove(&hash);
                warn!(
                    target: "neo",
                    index = next_index,
                    "drain_unverified: block persist failed"
                );
                break;
            }
        }

        if persisted > 0 {
            tracing::info!(
                target: "neo",
                persisted,
                current_height,
                "drain_unverified: persisted blocks from unverified cache"
            );
        }

        // If we hit the batch limit and the next block is available, schedule continuation
        if persisted >= DRAIN_BATCH_SIZE {
            let next_index = current_height.saturating_add(1);
            if self._block_cache_unverified.contains_key(&next_index) {
                let _ = ctx.self_ref().tell(BlockchainCommand::DrainUnverified);
            }
        }
    }

    /// Search the block cache (hash-keyed) for a block at the given index.
    /// This is a linear scan but only runs when the unverified cache misses,
    /// which is rare during normal operation.
    fn find_block_in_cache_by_index(
        &self,
        _context: &Arc<NeoSystemContext>,
        index: u32,
    ) -> Option<Arc<Block>> {
        for entry in self._block_cache.iter() {
            if entry.value().index() == index {
                let hash = *entry.key();
                drop(entry);
                return self._block_cache.remove(&hash).map(|(_, block)| block);
            }
        }
        None
    }

    /// Pop a single block from the unverified cache at the given index.
    fn pop_from_unverified(&self, index: u32) -> Option<Arc<Block>> {
        let (block, cost, empty) = {
            let mut entry = self._block_cache_unverified.get_mut(&index)?;
            let block = entry.blocks.pop()?;
            let cost = Self::unverified_block_cost(block.as_ref());
            (block, cost, entry.blocks.is_empty())
        };
        if empty {
            self._block_cache_unverified.remove(&index);
        }
        let _ = self._unverified_cache_bytes.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |current| Some(current.saturating_sub(cost)),
        );
        let _ = self._unverified_cache_blocks.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |current| Some(current.saturating_sub(1)),
        );
        Some(block)
    }

    pub(super) async fn on_new_extensible(&self, payload: ExtensiblePayload) -> VerifyResult {
        let Some(context) = &self.system_context else {
            return VerifyResult::Invalid;
        };

        let store_cache = context.store_cache();
        let settings = context.settings();
        let snapshot = store_cache.data_cache();

        self.ensure_extensible_witness_whitelist(settings.as_ref(), snapshot)
            .await;
        let whitelist = self._extensible_witness_white_list.read().await;

        if !payload.verify(settings.as_ref(), snapshot, &whitelist) {
            return VerifyResult::Invalid;
        }

        if payload.category == STATE_SERVICE_CATEGORY
            && let Err(err) = self.process_state_service_payload(context, &payload)
        {
            warn!(target: "neo", %err, "state service payload handling failed");
        }

        if let Err(err) = context.record_extensible(payload) {
            warn!(
                target: "neo",
                %err,
                "failed to record extensible payload"
            );
            return VerifyResult::Invalid;
        }
        VerifyResult::Succeed
    }

    async fn ensure_extensible_witness_whitelist(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
    ) {
        let needs_rebuild = self._extensible_witness_white_list.read().await.is_empty();
        if !needs_rebuild {
            return;
        }

        let rebuilt = Self::build_extensible_witness_whitelist(settings, snapshot);
        let mut whitelist = self._extensible_witness_white_list.write().await;
        if whitelist.is_empty() {
            *whitelist = rebuilt;
        }
    }

    fn build_extensible_witness_whitelist(
        settings: &ProtocolSettings,
        snapshot: &DataCache,
    ) -> std::collections::HashSet<UInt160> {
        use crate::smart_contract::Contract;
        use crate::smart_contract::native::helpers::NativeHelpers;
        use crate::smart_contract::native::{NeoToken, Role, RoleManagement};

        let current_height = LedgerContract::new().current_index(snapshot).unwrap_or(0);
        let mut whitelist = std::collections::HashSet::new();

        // Committee address (multi-sig).
        whitelist.insert(NativeHelpers::committee_address(settings, Some(snapshot)));

        // Consensus validators: BFT multi-sig address + individual signature contracts.
        let validators_count = usize::try_from(settings.validators_count.max(0)).unwrap_or(0);
        let validators = NeoToken::new()
            .get_next_block_validators_snapshot(snapshot, validators_count, settings)
            .unwrap_or_else(|_| settings.standby_validators());
        if !validators.is_empty() {
            whitelist.insert(NativeHelpers::get_bft_address(&validators));
            whitelist.extend(
                validators
                    .into_iter()
                    .map(|key| Contract::create_signature_contract(key).script_hash()),
            );
        }

        // State validators (optional): BFT multi-sig address + individual signature contracts.
        if let Ok(state_validators) = RoleManagement::new().get_designated_by_role_at(
            snapshot,
            Role::StateValidator,
            current_height,
        ) && !state_validators.is_empty()
        {
            whitelist.insert(NativeHelpers::get_bft_address(&state_validators));
            whitelist.extend(
                state_validators
                    .into_iter()
                    .map(|key| Contract::create_signature_contract(key).script_hash()),
            );
        }

        whitelist
    }

    pub(super) fn process_state_service_payload(
        &self,
        context: &Arc<NeoSystemContext>,
        payload: &ExtensiblePayload,
    ) -> Result<bool, CoreError> {
        if payload.data.is_empty() {
            return Ok(false);
        }

        // Neo.Plugins.StateService.Network.MessageType: Vote = 0, StateRoot = 1.
        if payload.data[0] != 1 {
            return Ok(false);
        }

        let mut reader = MemoryReader::new(&payload.data[1..]);
        let state_root = <StateRoot as Serializable>::deserialize(&mut reader)
            .map_err(|err| CoreError::invalid_data(err.to_string()))?;

        let Some(state_store) = context.state_store()? else {
            // State service is optional (plugin-like). When disabled, accept the payload but do not
            // attempt to validate or persist it.
            return Ok(true);
        };

        let accepted = state_store.on_new_state_root(state_root.clone());
        if accepted {
            context.actor_system.event_stream().publish(
                crate::state_service::ValidatedRootPersisted {
                    index: state_root.index,
                },
            );
        } else {
            debug!(
                target: "state",
                index = state_root.index,
                "state service payload rejected by StateStore"
            );
        }
        Ok(accepted)
    }
}
