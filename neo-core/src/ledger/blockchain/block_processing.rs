//
// block_processing.rs - Block processing logic for Blockchain actor
//

use super::*;

impl Blockchain {
    pub(super) async fn on_new_block(&self, block: &Block, verify: bool) -> VerifyResult {
        let Some(context) = &self.system_context else {
            return VerifyResult::Invalid;
        };

        let block_index = block.index();
        let hash = block.header.clone().hash();

        let store_cache = context.store_cache();
        let settings = context.settings();
        let header_cache = context.header_cache();

        let current_height = context.ledger().current_height();
        let header_height = header_cache
            .last()
            .map(|header| header.index())
            .unwrap_or(current_height);

        if block_index <= current_height {
            return VerifyResult::AlreadyExists;
        }

        if block_index > header_height + 1 {
            self.add_unverified_block(block.clone()).await;
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
                    tracing::warn!(
                        target: "neo",
                        index = block_index,
                        "header entry missing for block"
                    );
                    return VerifyResult::Invalid;
                };

                if header.hash() != hash {
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

        // Use write lock directly to prevent race condition where another
        // thread could insert the same block between read check and write insert.
        {
            let mut cache = self._block_cache.write().await;
            if cache.contains_key(&hash) {
                return VerifyResult::AlreadyExists;
            }
            cache.insert(hash, block.clone());
        }

        if block_index == current_height + 1 {
            if self.persist_block_sequence(block.clone()).await {
                VerifyResult::Succeed
            } else {
                VerifyResult::Invalid
            }
        } else {
            if block_index == header_height + 1 {
                header_cache.add(block.header.clone());
            }
            self.add_unverified_block(block.clone()).await;
            VerifyResult::Succeed
        }
    }

    async fn add_unverified_block(&self, block: Block) {
        let mut unverified = self._block_cache_unverified.write().await;
        let entry = unverified
            .entry(block.index())
            .or_insert_with(UnverifiedBlocksList::new);
        entry.blocks.push(block);
    }

    async fn persist_block_sequence(&self, block: Block) -> bool {
        let mut next_index = block.index().saturating_add(1);

        // Process the first block
        let first_succeeded = self.persist_block_via_system(&block);
        if first_succeeded {
            self.handle_persist_completed(PersistCompleted { block })
                .await;
        } else {
            return false;
        }

        // Process subsequent blocks from the unverified cache
        loop {
            let maybe_block = {
                let mut unverified = self._block_cache_unverified.write().await;
                if let Some(entry) = unverified.get_mut(&next_index) {
                    if let Some(next_block) = entry.blocks.pop() {
                        if entry.blocks.is_empty() {
                            unverified.remove(&next_index);
                        }
                        Some(next_block)
                    } else {
                        unverified.remove(&next_index);
                        None
                    }
                } else {
                    None
                }
            };

            let Some(next_block) = maybe_block else {
                break;
            };

            let succeeded = self.persist_block_via_system(&next_block);
            if succeeded {
                self.handle_persist_completed(PersistCompleted { block: next_block })
                    .await;
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

        if payload.category == STATE_SERVICE_CATEGORY {
            if let Err(err) = self.process_state_service_payload(context, &payload) {
                warn!(target: "neo", %err, "state service payload handling failed");
            }
        }

        context.record_extensible(payload);
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
        use crate::smart_contract::native::helpers::NativeHelpers;
        use crate::smart_contract::native::{NeoToken, Role, RoleManagement};
        use crate::smart_contract::Contract;

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
        ) {
            if !state_validators.is_empty() {
                whitelist.insert(NativeHelpers::get_bft_address(&state_validators));
                whitelist.extend(
                    state_validators
                        .into_iter()
                        .map(|key| Contract::create_signature_contract(key).script_hash()),
                );
            }
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
