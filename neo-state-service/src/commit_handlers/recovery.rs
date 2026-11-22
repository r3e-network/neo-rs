use super::*;
impl StateServiceCommitHandlers {
    /// Returns the root hash of the most recently persisted state trie.
    pub fn current_root_hash(&self) -> Option<UInt256> {
        *self.current_root.lock().unwrap()
    }

    /// Returns the persisted state-root record for `index`, if any.
    ///
    /// Records are readable immediately after persistence (through the shared
    /// store cache) and after restart (through the underlying store).
    pub fn get_state_root(&self, index: u32) -> Option<StateRoot> {
        read_state_root_record(&self.store_cache, index)
            .ok()
            .flatten()
    }

    /// Re-seeds the incremental trie baseline from persisted state-root records.
    ///
    /// After a restart the handler has no in-memory root, but root records and
    /// trie nodes persisted by earlier runs live in the store. Recovery adopts
    /// **only the locally committed root**:
    ///
    /// 1. the durable local cursor ([`PREFIX_LOCAL_ROOT_INDEX`]) names the last
    ///    height whose block this node actually committed, so an unsigned root
    ///    that arrived from the network for a block that does not exist locally
    ///    can never be adopted (S-1);
    /// 2. without a cursor (databases written before cursors existed) the
    ///    highest *unsigned* record is used, because only a record without a
    ///    witness proves this node computed the root itself;
    /// 3. the root's backing trie node must exist locally, otherwise extending
    ///    the trie would silently start from state that is not here.
    ///
    /// Returns the block index of the recovered root, or `None` when no valid
    /// local records exist.
    pub fn recover_from_storage(&self) -> Result<Option<u32>, StateServiceError> {
        let Some(index) = self.recoverable_local_root_index()? else {
            debug!(
                target: "neo::state_service",
                "no locally committed state roots found; starting from an empty trie"
            );
            return Ok(None);
        };

        let root = read_state_root_record(&self.store_cache, index)
            .map_err(StateServiceError::new)?
            .ok_or_else(|| {
                StateServiceError::new(format!(
                    "cannot recover state root: local progress names height {index} but no record is persisted"
                ))
            })?;

        if root.version != CURRENT_VERSION {
            return Err(StateServiceError::new(format!(
                "cannot recover state root: unsupported version {} at height {index}",
                root.version
            )));
        }

        self.ensure_root_node_is_durable(&root.root_hash, index)?;

        *self.current_root.lock().unwrap() = Some(root.root_hash);
        // Recovery deliberately starts from a HashNode. Only a root whose nodes
        // are known durable may become a warm baseline.
        *self.current_root_node.lock().unwrap() = None;
        *self.pending_root_node.lock().unwrap() = None;
        debug!(
            target: "neo::state_service",
            index,
            root = %root.root_hash,
            "recovered state root baseline from storage"
        );
        Ok(Some(index))
    }

    /// Returns the height whose root may be adopted at startup.
    ///
    /// The local cursor wins unconditionally: it is the only durable proof that
    /// the block backing the root was committed here. The legacy scan is a
    /// fallback for databases without a cursor and considers unsigned records
    /// only, since a signed record may describe a network height this node has
    /// not reached.
    fn recoverable_local_root_index(&self) -> Result<Option<u32>, StateServiceError> {
        if let Some(index) = read_local_root_cursor(&self.store_cache)? {
            return Ok(Some(index));
        }

        let prefix_key = StorageKey::new(STATE_ROOT_STORAGE_ID, vec![PREFIX_STATE_ROOT]);
        let mut highest: Option<u32> = None;
        for (key, item) in self
            .store_cache
            .find(Some(&prefix_key), SeekDirection::Forward)
        {
            let raw = key.key();
            if raw.len() != 5 || raw[0] != PREFIX_STATE_ROOT {
                // Unrelated entry inside the state-service namespace; skip it.
                continue;
            }
            let index = u32::from_be_bytes([raw[1], raw[2], raw[3], raw[4]]);
            let Some(record) = deserialize_state_root_record(index, item.value()) else {
                warn!(
                    target: "neo::state_service",
                    index,
                    "skipping malformed persisted state root record"
                );
                continue;
            };
            if record.witness.is_some() {
                // Quorum-validated network root: it proves nothing about local
                // block progress and must never seed the local trie.
                continue;
            }
            if highest.is_none_or(|current| index > current) {
                highest = Some(index);
            }
        }
        Ok(highest)
    }

    /// Fails unless the root node of `root_hash` is present in the local store.
    ///
    /// A root record can be persisted without its trie (a network root, a
    /// partially written batch, or pruned history). Adopting such a root makes
    /// every later trie operation fail or, worse, extend a nonexistent state.
    fn ensure_root_node_is_durable(
        &self,
        root_hash: &UInt256,
        index: u32,
    ) -> Result<(), StateServiceError> {
        // The node-key layout belongs to `neo-crypto`'s trie module; the state
        // service only namespaces whatever key the trie uses.
        let node = self
            .node_store
            .try_get(&node_key(root_hash))
            .map_err(|e| {
                StateServiceError::new(format!(
                    "cannot recover state root: failed to read trie node {root_hash} for height {index}: {e}"
                ))
            })?;
        if node.is_none() {
            return Err(StateServiceError::new(format!(
                "cannot recover state root: trie node {root_hash} for height {index} is missing from local storage"
            )));
        }
        Ok(())
    }

    /// Rolls back uncommitted state changes.
    ///
    /// This clears any pending state root that was computed but not yet
    /// persisted, typically called when block commit fails or is cancelled.
    /// The trie baseline is untouched (it only advances on successful
    /// persistence), so the next block re-derives its root from the last
    /// confirmed state.
    ///
    /// # Reference
    /// C# Neo StateService: rollback clears pending state changes
    pub fn rollback(&self) -> Result<(), StateServiceError> {
        if !self.state_root_enabled {
            return Ok(());
        }

        debug!(
            target: "neo::state_service",
            "rolling back pending state root"
        );

        // Discard both representations of the speculative block. The committed
        // hash and warm root remain untouched and still describe durable state.
        *self.pending_state_root.lock().unwrap() = None;
        *self.pending_root_node.lock().unwrap() = None;

        Ok(())
    }

    /// Finalizes a root whose record and MPT nodes committed with the block.
    ///
    /// The committed phase performs no storage writes: it only consumes the
    /// pending value, advances the in-memory trie baseline and returns the root
    /// for event publication.
    pub(super) fn persist_pending_state_root(
        &self,
        index: u32,
    ) -> Result<Option<StateRoot>, StateServiceError> {
        let state_root = {
            let mut pending = self.pending_state_root.lock().unwrap();
            pending.take()
        };
        let root_node = self.pending_root_node.lock().unwrap().take();

        let Some(state_root) = state_root else {
            return Ok(None);
        };

        if state_root.index != index {
            warn!(
                target: "neo::state_service",
                expected_index = index,
                actual_index = state_root.index,
                "state root index mismatch, skipping committed notification"
            );
            return Ok(None);
        }

        *self.current_root.lock().unwrap() = Some(state_root.root_hash);
        *self.current_root_node.lock().unwrap() = root_node;

        debug!(
            target: "neo::state_service",
            index,
            root_hash = %state_root.root_hash,
            "committed state root finalized"
        );

        Ok(Some(state_root))
    }

    /// Stages the local-root cursor in the same snapshot as the block and root record.
    pub(super) fn stage_local_root_progress(
        &self,
        context: Option<&dyn StateCommitContext>,
        snapshot: &PersistenceDataCache,
    ) -> Result<(), StateServiceError> {
        let Some(context) = context else {
            // Direct embedders without a system context retain the post-commit
            // fallback in StateRootStore::accept_local_root.
            return Ok(());
        };
        let Some(state_store) = context.state_store()? else {
            return Ok(());
        };
        let state_root = self
            .pending_state_root
            .lock()
            .unwrap()
            .as_ref()
            .cloned()
            .ok_or_else(|| StateServiceError::new("missing pending local state root"))?;
        if state_store.stage_local_root(&state_root, snapshot) {
            Ok(())
        } else {
            Err(StateServiceError::new(format!(
                "failed to stage local-root cursor at height {}",
                state_root.index
            )))
        }
    }

    /// Removes the in-memory staged marker when the block transaction aborts.
    pub(super) fn discard_staged_local_root(
        &self,
        context: Option<&dyn StateCommitContext>,
        index: u32,
    ) {
        if let Some(context) = context
            && let Ok(Some(state_store)) = context.state_store()
        {
            state_store.discard_staged_local_root(index);
        }
    }

    /// Notifies downstream consumers that a validated state root was persisted.
    ///
    /// Channels, in order:
    /// 1. the injected event sink (embedding/test seam),
    /// 2. the `NeoSystemContext` actor event stream plus its registered
    ///    `StateStoreService` (production wiring),
    /// 3. a warning when neither channel is available — never a silent skip.
    pub(super) fn announce_validated_root(
        &self,
        context: Option<&dyn StateCommitContext>,
        index: u32,
        state_root: &StateRoot,
    ) {
        let event = ValidatedRootPersisted { index };

        if let Some(sink) = &self.event_sink {
            sink(event.clone());
        }

        if let Some(context) = context {
            // Keep the registered state store service in sync with the new root.
            match context.state_store() {
                Ok(Some(state_store)) => {
                    let accepted = if state_root.witness.is_none() {
                        state_store.on_local_root_committed(state_root)
                    } else {
                        state_store.on_new_state_root(state_root.clone())
                    };
                    if !accepted {
                        warn!(
                            target: "neo::state_service",
                            index,
                            "state store service rejected the persisted state root"
                        );
                    }
                }
                Ok(None) => {
                    debug!(
                        target: "neo::state_service",
                        index,
                        "no state store service registered; skipping on_new_state_root notification"
                    );
                }
                Err(err) => {
                    warn!(
                        target: "neo::state_service",
                        index,
                        error = ?err,
                        "failed to resolve the state store service"
                    );
                }
            }
            context.publish_validated_root_persisted(event);
        } else if self.event_sink.is_none() {
            warn!(
                target: "neo::state_service",
                index,
                "ValidatedRootPersisted dropped: no event sink attached and the system context could not be resolved"
            );
        }
    }
}
