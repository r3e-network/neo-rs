use super::trie_stores::{BlockTrieNodeStore, STATE_ROOT_NODE_CACHE_CAPACITY};
use super::*;
impl StateServiceCommitHandlers {
    /// Creates a new StateServiceCommitHandlers instance.
    ///
    /// # Arguments
    /// * `store_cache` - Store cache for persisting state roots and trie nodes
    /// * `state_root_enabled` - Whether to compute and track state roots
    ///
    /// # Reference
    /// C# Neo StatePlugin constructor initializes event handlers
    pub fn new(
        store_cache: Arc<StoreCache>,
        state_root_enabled: bool,
    ) -> Result<Self, StateServiceError> {
        Ok(Self {
            node_store: Arc::new(TrieNodeStore {
                store_cache: store_cache.clone(),
            }),
            store_cache,
            state_root_enabled,
            pending_state_root: Arc::new(Mutex::new(None)),
            pending_root_node: Arc::new(Mutex::new(None)),
            node_cache: Arc::new(Mutex::new(state_root_enabled.then(|| {
                Arc::new(GlobalNodeCache::with_capacity(
                    NonZeroUsize::new(STATE_ROOT_NODE_CACHE_CAPACITY)
                        .expect("state-root node cache capacity is nonzero"),
                ))
            }))),
            node_cache_enabled: Arc::new(AtomicBool::new(true)),
            current_root: Arc::new(Mutex::new(None)),
            current_root_node: Arc::new(Mutex::new(None)),
            event_sink: None,
            profile_sink: Arc::new(Mutex::new(None)),
        })
    }

    /// Enables or bypasses compact-node caching for subsequent trie operations.
    /// Disabling does not clear existing entries or reset statistics.
    pub fn set_node_cache_enabled(&self, enabled: bool) {
        self.node_cache_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Replaces the cache with an empty bounded cache for an explicit cold A/B run.
    /// Content-addressed persistent nodes and the committed root are unaffected.
    pub fn reset_node_cache_capacity(&self, capacity: NonZeroUsize) {
        let mut cache = self.node_cache.lock().unwrap();
        if cache.is_some() {
            *cache = Some(Arc::new(GlobalNodeCache::with_capacity(capacity)));
        }
    }

    /// Returns cache activity without changing execution or eviction state.
    #[must_use]
    pub fn node_cache_stats(&self) -> StateRootNodeCacheStats {
        let cache = self.node_cache.lock().unwrap().clone();
        let (hits, misses, hit_rate_percent) =
            cache.as_ref().map_or((0, 0, 0.0), |cache| cache.stats());
        let (warm_root_nodes, warm_root_estimated_bytes) = self
            .current_root_node
            .lock()
            .unwrap()
            .as_ref()
            .map_or((0, 0), neo_crypto::Node::resolved_retention_stats);
        StateRootNodeCacheStats {
            enabled: cache.is_some() && self.node_cache_enabled.load(Ordering::Relaxed),
            capacity: cache.as_ref().map_or(0, |cache| cache.capacity()),
            entries: cache.as_ref().map_or(0, |cache| cache.len()),
            hits,
            misses,
            hit_rate_percent,
            warm_root_nodes,
            warm_root_estimated_bytes,
        }
    }

    fn active_node_cache(&self) -> Option<Arc<GlobalNodeCache>> {
        if self.node_cache_enabled.load(Ordering::Relaxed) {
            self.node_cache.lock().unwrap().clone()
        } else {
            None
        }
    }

    /// Attaches or removes the observation-only state-root timing sink.
    pub fn set_profile_sink(&self, sink: Option<StateRootProfileSink>) {
        *self.profile_sink.lock().unwrap() = sink;
    }

    /// Returns whether a state-root timing sink is currently attached.
    ///
    /// This is observation-only introspection used to prove the embedding
    /// wired profiling; it never enables profiling by itself.
    #[must_use]
    pub fn profile_sink_registered(&self) -> bool {
        self.profile_sink.lock().unwrap().is_some()
    }

    /// Attaches an observer invoked whenever a validated state root is
    /// persisted and announced.
    ///
    /// This is the embedding seam for consumers that cannot be reached through
    /// the `NeoSystemContext` event stream (e.g. tests and embedders).
    #[must_use]
    pub fn with_event_sink(
        mut self,
        sink: Arc<dyn Fn(ValidatedRootPersisted) + Send + Sync>,
    ) -> Self {
        self.event_sink = Some(sink);
        self
    }

    /// Commits state changes for a block by computing its state root.
    ///
    /// This method:
    /// 1. Extracts the block's storage changes from the snapshot
    /// 2. Applies them to the incremental MPT trie extending the previously
    ///    persisted root
    /// 3. Stages the resulting trie nodes and root record in this block's snapshot
    /// 4. Stores the root as pending, awaiting the committed handler
    ///
    /// A block with no storage changes keeps the previous root unchanged. A
    /// block with no changes and no previous root is rejected: an empty state
    /// trie has no representable root in this MPT port.
    ///
    /// # Arguments
    /// * `height` - Block height to compute state root for
    /// * `snapshot` - Data cache carrying the block's storage changes
    ///
    /// # Reference
    /// C# Neo StateService: StatePlugin.OnCommitting computes state root from snapshot
    pub fn commit_block(
        &self,
        height: u32,
        snapshot: &PersistenceDataCache,
    ) -> Result<(), StateServiceError> {
        self.commit_block_with_parallelism(height, snapshot, true)
    }

    pub(super) fn commit_block_with_parallelism(
        &self,
        height: u32,
        snapshot: &PersistenceDataCache,
        parallel_hash: bool,
    ) -> Result<(), StateServiceError> {
        if !self.state_root_enabled {
            return Ok(());
        }

        debug!(
            target: "neo::state_service",
            height,
            "computing state root for block"
        );

        let profile_sink = self.profile_sink.lock().unwrap().clone();
        let extraction_started = profile_sink.as_ref().map(|_| Instant::now());
        let mpt_profile = profile_sink
            .as_ref()
            .map(|_| Arc::new(MptProfile::default()));

        // Capture contract changes before adding state-service entries to the snapshot.
        // Neo's MPT commits contract storage only. LedgerContract indexes (-4)
        // are chain bookkeeping and are deliberately excluded by both C# and Go.
        let mut changes = snapshot
            .extract_raw_changes()
            .into_iter()
            .filter(|(key, _)| key.len() < 4 || key[..4] != (-4_i32).to_le_bytes())
            .collect::<Vec<_>>();
        // PersistenceDataCache uses a HashMap, so its iteration order randomly jumps around
        // the trie. Lexicographic storage-key order is also MPT nibble-path order:
        // adjacent updates can then reuse the path resolved by the previous put.
        // Keys are unique and MPT updates are order-independent for the final root.
        changes.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        let change_extraction = extraction_started.map(|started| started.elapsed());
        let mpt_started = profile_sink.as_ref().map(|_| Instant::now());
        let (root_hash, root_node) =
            self.compute_state_root(snapshot, &changes, mpt_profile.clone(), parallel_hash)?;
        let mpt = mpt_started.map(|started| started.elapsed());
        let state_root = StateRoot::new_current(height, root_hash);
        snapshot
            .try_add(
                state_root_record_key(height),
                StorageItem::from_bytes(serialize_state_root_record(&state_root)),
            )
            .map_err(|error| {
                StateServiceError::new(format!(
                    "failed to stage state root at index {height}: {error}"
                ))
            })?;
        *self.pending_state_root.lock().unwrap() = Some(state_root);
        *self.pending_root_node.lock().unwrap() = Some(root_node);
        if let (Some(sink), Some(change_extraction), Some(mpt)) =
            (profile_sink, change_extraction, mpt)
        {
            sink(StateRootCommitProfile {
                change_extraction,
                mpt,
                mpt_detail: mpt_profile
                    .as_deref()
                    .map(MptProfile::snapshot)
                    .unwrap_or_default(),
            });
        }

        debug!(
            target: "neo::state_service",
            height,
            %root_hash,
            changes = changes.len(),
            "state root computed and staged successfully"
        );

        Ok(())
    }

    /// Computes the state root hash by applying the given storage changes to
    /// the incremental MPT trie.
    ///
    /// The trie extends the previously persisted root, so historical roots
    /// remain provable (shared nodes are never removed: `full_state = true`).
    ///
    /// # Reference
    /// C# Neo StateService: GetLocalRootHash() - computes MPT root from snapshot
    fn compute_state_root(
        &self,
        snapshot: &PersistenceDataCache,
        changes: &[(Vec<u8>, Option<Vec<u8>>)],
        profile: Option<Arc<MptProfile>>,
        parallel_hash: bool,
    ) -> Result<(UInt256, neo_crypto::mpt_trie::Node), StateServiceError> {
        let previous_root = *self.current_root.lock().unwrap();

        if changes.is_empty() && previous_root.is_none() {
            return Err(StateServiceError::new(
                "cannot compute a state root: the state trie is empty \
                 (no previous root and no storage changes in this block)",
            ));
        }

        let block_store = Arc::new(BlockTrieNodeStore {
            persisted: self.node_store.clone(),
            snapshot: snapshot.clone(),
        });
        // Cloning retains immutable Arc subtrees. Trie updates detach every node
        // on the modified path, so an aborted block cannot mutate this baseline.
        let root_node = self.current_root_node.lock().unwrap().clone();
        let root_node = root_node.unwrap_or_else(|| {
            previous_root.map_or_else(neo_crypto::mpt_trie::Node::new, |root| {
                neo_crypto::mpt_trie::Node::new_hash(root)
            })
        });
        let mut trie = Trie::new_with_root_node(
            block_store,
            root_node,
            // Full-state mode retains nodes shared with older roots, which is
            // required for proof generation against historical roots.
            true,
            self.active_node_cache(),
        );
        if let Some(profile) = profile {
            trie = trie.with_profile(profile);
        }

        for (key, value) in changes {
            match value {
                Some(value) => trie.put(key, value).map_err(|e| {
                    StateServiceError::new(format!("MPT put failed for storage key: {e}"))
                })?,
                None => {
                    trie.delete(key).map_err(|e| {
                        StateServiceError::new(format!("MPT delete failed for storage key: {e}"))
                    })?;
                }
            }
        }

        // Flush the new/changed nodes into the shared node store so they commit
        // atomically with this block.
        trie.commit_with_parallelism(parallel_hash)
            .map_err(|e| StateServiceError::new(format!("MPT commit failed: {e}")))?;

        let root_hash = trie.root_hash().ok_or_else(|| {
            StateServiceError::new(
                "cannot compute a state root: applying this block left the state trie empty",
            )
        })?;
        Ok((root_hash, trie.take_root()))
    }
}
