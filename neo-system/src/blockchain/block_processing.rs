//
// block_processing.rs - Block processing logic for Blockchain actor
//

use super::*;
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
            .map_or(current_height, |header| header.index());

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

        // Body-integrity checks are state-independent and, unlike the `verify`
        // flag, always apply: block identity is the header hash, so a peer that
        // reuses a genuine signed header with a substituted transaction list
        // would otherwise reach persistence (and be relayed) with a merkle root
        // that commits to nothing in the node's state.
        //
        // Only the wire rules are enforced here (version, one header witness,
        // at most 65,535 transactions, no duplicates, merkle root). The
        // 512-transaction and 2 MiB limits are consensus *proposal* policy in
        // C# and are not checked on acceptance: a block the validators signed
        // must be accepted whatever its size, or this node stalls on a chain
        // every other node follows.
        let payload_block = match neo_ledger::verify_execution_block_integrity(block.as_ref()) {
            Ok(payload_block) => payload_block,
            Err(error) => {
                tracing::warn!(
                    target: "neo",
                    index = block_index,
                    %hash,
                    %error,
                    "block failed body integrity check"
                );
                return VerifyResult::Invalid;
            }
        };

        if block_index > header_height + 1 {
            self.add_unverified_block(Arc::clone(&block)).await;
            return VerifyResult::UnableToVerify;
        }

        if verify {
            if block_index == header_height + 1 {
                // Convert to payload block for validation
                if !neo_ledger::verify_block_with_cache(
                    &payload_block,
                    settings.as_ref(),
                    &store_cache,
                    &header_cache,
                ) {
                    tracing::warn!(
                        target: "neo",
                        index = block_index,
                        %hash,
                        prev = %block.header.previous_hash,
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

        if self.cache.contains_verified(&hash) {
            return VerifyResult::AlreadyExists;
        }

        if block_index == current_height + 1 {
            // The chain needs this block now. It is persisted whether or not
            // the cache budget has room for it; nothing is evicted to keep an
            // entry that only marks the tip once the block is persisted.
            if let Some(retained) = self.cache.try_retain(Arc::clone(&block), hash) {
                self.cache.insert_verified(retained);
            }
            return if self.persist_block_sequence(Arc::clone(&block)).await {
                VerifyResult::Succeed
            } else {
                // Persistence failed â€” remove from block cache so the block
                // can be retried if it arrives again (e.g. from another peer).
                self.cache.remove_verified(&hash);
                VerifyResult::Invalid
            };
        }

        if self.cache.verified_len() >= MAX_BLOCK_CACHE_SIZE {
            // Cache is full but this isn't the next block to persist.
            // Park it in the unverified cache so persist_block_sequence
            // can pick it up later, and return OutOfMemory (not Invalid)
            // so the task manager does not disconnect the peer.
            self.add_unverified_block(Arc::clone(&block)).await;
            return VerifyResult::OutOfMemory;
        }

        let Some(retained) = self
            .cache
            .retain(Arc::clone(&block), hash, self.ledger.as_ref())
        else {
            // The budget is held by blocks the chain needs sooner than this
            // one, or the block is larger than the whole budget. It is not
            // kept and has to be fetched again; OutOfMemory (not Invalid)
            // keeps the peer from being blamed for it.
            tracing::debug!(
                target: "neo",
                index = block_index,
                %hash,
                "block cache budget cannot hold verified block, not caching it"
            );
            return VerifyResult::OutOfMemory;
        };
        self.cache.insert_verified(retained.clone());

        if block_index == header_height + 1 {
            header_cache.add(payload_block.header);
        }
        // Parked under the reservation the verified cache entry already
        // holds, so the block is charged once.
        self.cache
            .admit_unverified(block, hash, Some(retained), self.ledger.as_ref());
        VerifyResult::Succeed
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
        self.cache
            .admit_unverified(block, block_hash, None, self.ledger.as_ref());
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

            let Some(next) = self.cache.pop_unverified(next_index) else {
                break;
            };
            let next_block = Arc::clone(&next.block);

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
                // The new tip keeps the charge the block was parked with.
                if !self.cache.contains_verified(&next.hash) {
                    self.cache.insert_verified(next);
                }
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
            let next = self
                .cache
                .pop_unverified(next_index)
                .or_else(|| self.cache.take_verified_at_index(next_index));
            let Some(next) = next else {
                break;
            };
            let block = Arc::clone(&next.block);

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

            // Insert into block_cache for dedup only after verification. The
            // entry keeps the charge the block was cached with.
            let hash = next.hash;
            self.cache.insert_verified(next);

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
                self.cache.remove_verified(&hash);
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
            if self.cache.contains_unverified_height(next_index) {
                let _ = ctx.self_ref().tell(BlockchainCommand::DrainUnverified);
            }
        }
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
        snapshot: &PersistenceDataCache,
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
        snapshot: &PersistenceDataCache,
    ) -> std::collections::HashSet<UInt160> {
        use neo_native_contracts::helpers::NativeHelpers;
        use neo_native_contracts::{NeoToken, Role, RoleManagement};
        use neo_smart_contract::Contract;

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
