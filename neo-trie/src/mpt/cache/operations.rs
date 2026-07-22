use super::telemetry::{elapsed_ns, elapsed_us, process_resource_snapshot};
use super::*;
use rustc_hash::FxHashSet;
use std::time::Instant;

impl<S> MptCache<S>
where
    S: MptStoreSnapshot + 'static,
{
    /// Resolves the node identified by the supplied hash if present either in the
    /// in-memory cache or the underlying store.
    pub fn resolve(&mut self, hash: &UInt256) -> MptResult<Option<Node>> {
        let entry = self.resolve_internal(hash)?;
        let mut node = entry.resolve_clone()?;
        if let Some(node) = node.as_mut() {
            // `Node::clone` intentionally clears the materialized node's own
            // memoized hash like C#. Keep the cache lookup key separately as
            // pruning provenance without changing subsequent `hash()` calls.
            node.set_accounted_hash(*hash);
        }
        Ok(node)
    }

    /// Adds or updates the supplied node inside the cache.
    pub fn put_node(&mut self, mut node: Node) -> MptResult<()> {
        let payload_without_reference = node.to_array_without_reference()?;
        super::super::metrics::record_hash_computation();
        let hash_bytes = neo_crypto::Crypto::hash256(&payload_without_reference);
        let hash = UInt256::from_bytes(&hash_bytes).map_err(MptError::from)?;
        node.set_finalized_hash(hash);
        let node_type = node.node_type;
        self.put_node_with_payload(Some(node), node_type, hash, payload_without_reference)
    }

    /// Adds or updates the supplied node inside the cache while keeping the
    /// caller's node hash cached.
    pub(crate) fn put_node_cached(&mut self, node: &mut Node) -> MptResult<()> {
        let repeated_ancestor = matches!(
            node.node_type,
            NodeType::BranchNode | NodeType::ExtensionNode
        ) && node.cached_hash().is_some_and(|hash| {
            self.entries
                .get(&hash)
                .is_some_and(|entry| entry.produced_in_current_commit)
        });
        node.set_dirty();
        self.put_node_cached_inner(node, repeated_ancestor)
    }

    pub(crate) fn prepare_node_finalization(
        &mut self,
        node: &mut Node,
    ) -> MptResult<PendingNodeFinalization> {
        let payload_without_reference = node.to_array_without_reference()?;
        self.mutation_stats.put_node_cached_calls =
            self.mutation_stats.put_node_cached_calls.saturating_add(1);
        self.mutation_stats.serialized_payload_bytes = self
            .mutation_stats
            .serialized_payload_bytes
            .saturating_add(payload_without_reference.len() as u64);
        let hash = if let Some(hash) = node.cached_hash() {
            hash
        } else {
            super::super::metrics::record_hash_computation();
            let hash_bytes = neo_crypto::Crypto::hash256(&payload_without_reference);
            UInt256::from_bytes(&hash_bytes).map_err(MptError::from)?
        };
        node.set_pending_hash(hash);
        Ok(PendingNodeFinalization {
            node_type: node.node_type,
            hash,
            payload_without_reference,
        })
    }

    pub(crate) fn finalize_prepared_nodes(
        &mut self,
        pending: Vec<PendingNodeFinalization>,
    ) -> MptResult<()> {
        if self.defer_reference_resolution {
            return self.defer_prepared_nodes(pending);
        }

        let mut new_hashes = FxHashSet::default();
        new_hashes.reserve(pending.len());
        let mut missing_hashes = Vec::new();
        let mut cache_hits = 0u64;
        for node in &pending {
            if self.entries.contains_key(&node.hash) || !new_hashes.insert(node.hash) {
                cache_hits = cache_hits.saturating_add(1);
            } else {
                missing_hashes.push(node.hash);
            }
        }
        self.mutation_stats.finalization_cache_hits = self
            .mutation_stats
            .finalization_cache_hits
            .saturating_add(cache_hits);

        missing_hashes.sort_unstable_by_key(UInt256::to_array);
        let keys = missing_hashes
            .iter()
            .map(|hash| node_key_bytes(self.prefix, hash).to_vec())
            .collect::<Vec<_>>();
        let lookups = match self.store.try_get_nodes_with_source(&keys) {
            Ok(lookups) => lookups,
            Err(error) => {
                self.mutation_stats.finalization_lookup_errors = self
                    .mutation_stats
                    .finalization_lookup_errors
                    .saturating_add((pending.len() as u64).saturating_sub(cache_hits));
                return Err(error);
            }
        };
        if lookups.len() != missing_hashes.len() {
            self.mutation_stats.finalization_lookup_errors = self
                .mutation_stats
                .finalization_lookup_errors
                .saturating_add((pending.len() as u64).saturating_sub(cache_hits));
            return Err(MptError::storage(format!(
                "MPT batch lookup returned {} results for {} hashes",
                lookups.len(),
                missing_hashes.len()
            )));
        }

        let mut loaded = Vec::with_capacity(lookups.len());
        for (hash, lookup) in missing_hashes.into_iter().zip(lookups) {
            let mut node = match lookup {
                MptStoreLookup::InMemory(node) => {
                    if node.is_some() {
                        self.mutation_stats.finalization_memory_hits = self
                            .mutation_stats
                            .finalization_memory_hits
                            .saturating_add(1);
                    } else {
                        self.mutation_stats.finalization_memory_misses = self
                            .mutation_stats
                            .finalization_memory_misses
                            .saturating_add(1);
                    }
                    node
                }
                MptStoreLookup::Backing(node) => {
                    if node.is_some() {
                        self.mutation_stats.finalization_backing_hits = self
                            .mutation_stats
                            .finalization_backing_hits
                            .saturating_add(1);
                    } else {
                        self.mutation_stats.finalization_backing_misses = self
                            .mutation_stats
                            .finalization_backing_misses
                            .saturating_add(1);
                    }
                    node
                }
            };
            if let Some(node) = node.as_mut() {
                node.set_accounted_hash(hash);
            }
            loaded.push((hash, MptTrackable::new(node)));
        }
        for (hash, entry) in loaded {
            self.entries.insert(hash, entry);
        }

        for node in pending {
            let entry = self
                .entries
                .get_mut(&node.hash)
                .ok_or_else(|| MptError::invalid("prefetched MPT finalization entry is missing"))?;
            Self::stage_payload(entry, node.node_type, node.payload_without_reference);
        }
        Ok(())
    }

    pub(crate) fn defer_intermediate_node(&mut self, node: &mut Node) -> MptResult<()> {
        let pending = self.prepare_node_finalization(node)?;
        let cache_hit = self.defer_prepared_node(pending);
        self.mutation_stats.finalization_cache_hits = self
            .mutation_stats
            .finalization_cache_hits
            .saturating_add(u64::from(cache_hit));
        Ok(())
    }

    pub(crate) const fn defers_intermediate_nodes(&self) -> bool {
        self.defer_intermediate_nodes
    }

    fn defer_prepared_nodes(&mut self, pending: Vec<PendingNodeFinalization>) -> MptResult<()> {
        let mut cache_hits = 0u64;
        for node in pending {
            cache_hits = cache_hits.saturating_add(u64::from(self.defer_prepared_node(node)));
        }
        self.mutation_stats.finalization_cache_hits = self
            .mutation_stats
            .finalization_cache_hits
            .saturating_add(cache_hits);
        Ok(())
    }

    fn defer_prepared_node(&mut self, node: PendingNodeFinalization) -> bool {
        if let Some(entry) = self.entries.get_mut(&node.hash) {
            Self::stage_payload(entry, node.node_type, node.payload_without_reference);
            return true;
        }

        self.append_deferred_operation(
            node.hash,
            DeferredReferenceOperation::Put {
                node_type: node.node_type,
                payload_without_reference: node.payload_without_reference,
            },
        )
    }

    fn put_node_cached_inner(&mut self, node: &mut Node, repeated_ancestor: bool) -> MptResult<()> {
        let payload_without_reference = node.to_array_without_reference()?;
        self.mutation_stats.put_node_cached_calls =
            self.mutation_stats.put_node_cached_calls.saturating_add(1);
        self.mutation_stats.serialized_payload_bytes = self
            .mutation_stats
            .serialized_payload_bytes
            .saturating_add(payload_without_reference.len() as u64);
        if repeated_ancestor {
            self.mutation_stats.repeated_ancestor_finalizations = self
                .mutation_stats
                .repeated_ancestor_finalizations
                .saturating_add(1);
        }
        let hash = if let Some(hash) = node.cached_hash() {
            hash
        } else {
            super::super::metrics::record_hash_computation();
            let hash_bytes = neo_crypto::Crypto::hash256(&payload_without_reference);
            UInt256::from_bytes(&hash_bytes).map_err(MptError::from)?
        };
        self.put_node_with_payload(None, node.node_type, hash, payload_without_reference)?;
        node.set_finalized_hash(hash);
        Ok(())
    }

    fn put_node_with_payload(
        &mut self,
        node: Option<Node>,
        node_type: NodeType,
        hash: UInt256,
        payload_without_reference: Vec<u8>,
    ) -> MptResult<()> {
        let entry = self.resolve_finalized(&hash)?;

        Self::stage_payload(entry, node_type, payload_without_reference);
        if let Some(mut node) = node {
            node.reference = entry.reference;
            entry.node = Some(node);
        }
        Ok(())
    }

    fn stage_payload(
        entry: &mut MptTrackable,
        node_type: NodeType,
        payload_without_reference: Vec<u8>,
    ) {
        if entry.node_type.is_some() {
            entry.reference = entry.reference.wrapping_add(1);
            entry.state = TrackState::Changed;
        } else {
            entry.reference = 1;
            entry.state = TrackState::Added;
        }
        entry.node_type = Some(node_type);
        if let Some(existing) = entry.node.as_mut() {
            existing.reference = entry.reference;
        }
        entry.payload_without_reference = Some(payload_without_reference);
        entry.produced_in_current_commit = true;
    }

    pub(crate) fn record_hash_computations(&mut self, count: u64) {
        self.mutation_stats.hash_computations =
            self.mutation_stats.hash_computations.saturating_add(count);
    }

    pub(crate) fn record_mutation_depth(&mut self, depth: usize) {
        self.mutation_stats.max_recursion_depth =
            self.mutation_stats.max_recursion_depth.max(depth as u64);
    }

    pub(crate) const fn mutation_stats(&self) -> MptMutationStats {
        self.mutation_stats
    }

    pub(crate) fn take_mutation_stats(&mut self) -> MptMutationStats {
        std::mem::take(&mut self.mutation_stats)
    }

    /// Decrements the reference count for the node or marks it for deletion when it
    /// is no longer referenced.
    pub fn delete_node(&mut self, hash: UInt256) -> MptResult<()> {
        if self.defer_reference_resolution && self.deferred_entries.contains_key(&hash) {
            self.append_deferred_operation(hash, DeferredReferenceOperation::Delete);
            return Ok(());
        }

        let entry = self.resolve_internal(&hash)?;
        if entry.node_type.is_none() {
            return Ok(());
        }
        if entry.reference > 1 {
            entry.reference -= 1;
            if entry.payload_without_reference.is_none() {
                let node = entry.node.as_ref().ok_or_else(|| {
                    MptError::invalid("cache entry cannot serialize without a materialized node")
                })?;
                entry.payload_without_reference = Some(node.to_array_without_reference()?);
            }
            if let Some(node) = entry.node.as_mut() {
                node.reference = entry.reference;
            }
            entry.state = TrackState::Changed;
        } else {
            entry.node = None;
            entry.node_type = None;
            entry.reference = 0;
            entry.payload_without_reference = None;
            entry.state = TrackState::Deleted;
        }
        Ok(())
    }

    pub(crate) fn checkpoint(&mut self) {
        for entry in self.entries.values_mut() {
            entry.produced_in_current_commit = false;
        }
    }

    /// Enables or disables unresolved deferred-journal export at commit time.
    ///
    /// Only meaningful for deferred full-state batch tries whose store stages
    /// unresolved journals (see
    /// [`MptStoreSnapshot::stage_unresolved_deferred_journal`]); every other
    /// configuration keeps the classic resolve-then-write flow.
    pub fn set_deferred_journal_export(&mut self, enabled: bool) {
        self.export_deferred_journal = enabled;
    }

    /// Flushes the pending changes to the underlying store.
    pub fn commit(&mut self) -> MptResult<()> {
        let mut overlay = Vec::with_capacity(
            self.entries
                .len()
                .saturating_add(self.deferred_entries.len()),
        );
        for (hash, entry) in &self.entries {
            match entry.state {
                TrackState::None => {}
                TrackState::Added | TrackState::Changed => {
                    let node_type = entry
                        .node_type
                        .ok_or_else(|| MptError::invalid("cache entry missing node type"))?;
                    let payload_without_reference =
                        entry.payload_without_reference.as_ref().ok_or_else(|| {
                            MptError::invalid("cache entry missing serialized node payload")
                        })?;
                    let data = Node::array_from_payload_parts(
                        node_type,
                        entry.reference,
                        payload_without_reference,
                    )?;
                    overlay.push((self.key(hash), Some(data)));
                }
                TrackState::Deleted => {
                    overlay.push((self.key(hash), None));
                }
            }
        }

        // Fused commit: when requested, hand the deferred full-state journal
        // to the store unresolved so the backing commit can resolve reference
        // counts at its write cursor. The materialized overlay above carries
        // no deferred entries, and its keys are disjoint from the journaled
        // hashes (a hash lives in either `entries` or `deferred_entries`,
        // never both), so cursor resolution observes exactly the base the
        // classic snapshot probe would have seen.
        let exported_journal = if self.export_deferred_journal {
            self.summarize_deferred_journal()
        } else {
            None
        };
        match exported_journal {
            Some(journal) if !journal.is_empty() => {
                // Byte parity rests on journal keys being disjoint from the
                // materialized overlay: the fused cursor resolves a journaled
                // hash against the pre-overlay base exactly because no overlay
                // entry can carry the same key.
                debug_assert!(
                    journal
                        .iter()
                        .all(|entry| !overlay.iter().any(|(key, _)| key == &entry.key)),
                    "deferred journal keys must be disjoint from the materialized overlay"
                );
                if self.store.stage_unresolved_deferred_journal(journal)? {
                    self.store.apply_overlay(overlay)?;
                } else {
                    // The store cannot carry an unresolved journal; resolve
                    // against the backing snapshot exactly like the classic
                    // path. The deferred journal is untouched, so the replay
                    // below sees the same operations it always would.
                    overlay.extend(self.deferred_overlay()?);
                    self.store.apply_overlay(overlay)?;
                }
            }
            _ => {
                overlay.extend(self.deferred_overlay()?);
                self.store.apply_overlay(overlay)?;
            }
        }
        self.entries.clear();
        self.deferred_entries.clear();
        self.deferred_operations.clear();
        Ok(())
    }

    /// Summarizes the deferred journal into per-hash put counts and first-put
    /// payloads, ordered by storage key, without consulting the backing store.
    ///
    /// Returns `None` when the journal contains a delete, which a put-count
    /// summary cannot represent. Full-state tries never record deletes
    /// (`Trie::previous_hash` yields `None` in full-state mode), so `None`
    /// signals an unexpected journal whose caller must fall back to the
    /// classic resolve-then-write path, which handles deletes.
    fn summarize_deferred_journal(&mut self) -> Option<Vec<UnresolvedDeferredNode>> {
        if self.deferred_entries.is_empty() {
            return Some(Vec::new());
        }

        let stage_start = Instant::now();
        let mut journal = Vec::with_capacity(self.deferred_entries.len());
        for (hash, entry) in &self.deferred_entries {
            let mut summary: Option<UnresolvedDeferredNode> = None;
            let mut current = Some(entry.first);
            while let Some(index) = current {
                let record = self.deferred_operations.get(index)?;
                match &record.operation {
                    DeferredReferenceOperation::Put {
                        node_type,
                        payload_without_reference,
                    } => match summary.as_mut() {
                        Some(summary) => {
                            summary.puts = summary.puts.checked_add(1)?;
                        }
                        None => {
                            summary = Some(UnresolvedDeferredNode {
                                key: Self::key_for(self.prefix, hash),
                                node_type: *node_type,
                                payload_without_reference: payload_without_reference.clone(),
                                puts: 1,
                            });
                        }
                    },
                    DeferredReferenceOperation::Delete => return None,
                }
                current = record.next;
            }
            journal.push(summary?);
        }
        journal.sort_unstable_by(|left, right| left.key.cmp(&right.key));
        self.mutation_stats.deferred_finalization_prepare_us = self
            .mutation_stats
            .deferred_finalization_prepare_us
            .saturating_add(elapsed_us(stage_start));
        Some(journal)
    }

    fn deferred_overlay(&mut self) -> MptResult<Vec<(Vec<u8>, Option<Vec<u8>>)>> {
        if self.deferred_entries.is_empty() {
            return Ok(Vec::new());
        }

        let stage_start = Instant::now();
        let mut pending = self
            .deferred_entries
            .iter()
            .map(|(hash, entry)| (*hash, *entry))
            .collect::<Vec<_>>();
        pending.sort_unstable_by_key(|(hash, _)| hash.to_array());
        let keys = pending
            .iter()
            .map(|(hash, _)| node_key_bytes(self.prefix, hash))
            .collect::<Vec<_>>();
        self.mutation_stats.deferred_finalization_prepare_us = self
            .mutation_stats
            .deferred_finalization_prepare_us
            .saturating_add(elapsed_us(stage_start));

        let stage_start = Instant::now();
        // The deferred path issues only a handful of sorted batches per
        // commit. Capture process I/O around the provider call itself so the
        // evidence does not add work to ordinary point resolution.
        let resources_before = process_resource_snapshot();
        let lookup_result = self.store.try_get_nodes_with_source_raw_borrowed(&keys);
        let resources_after = process_resource_snapshot();
        self.mutation_stats
            .record_deferred_resource_delta(resources_before, resources_after);
        let lookups = match lookup_result {
            Ok(lookups) => lookups,
            Err(error) => {
                self.mutation_stats.deferred_finalization_lookup_us = self
                    .mutation_stats
                    .deferred_finalization_lookup_us
                    .saturating_add(elapsed_us(stage_start));
                self.mutation_stats.finalization_lookup_errors = self
                    .mutation_stats
                    .finalization_lookup_errors
                    .saturating_add(pending.len() as u64);
                return Err(error);
            }
        };
        self.mutation_stats.deferred_finalization_lookup_us = self
            .mutation_stats
            .deferred_finalization_lookup_us
            .saturating_add(elapsed_us(stage_start));
        if lookups.len() != pending.len() {
            self.mutation_stats.finalization_lookup_errors = self
                .mutation_stats
                .finalization_lookup_errors
                .saturating_add(pending.len() as u64);
            return Err(MptError::storage(format!(
                "MPT deferred batch lookup returned {} results for {} hashes",
                lookups.len(),
                pending.len()
            )));
        }

        let mut overlay = Vec::with_capacity(pending.len());
        for ((hash, entry), lookup) in pending.into_iter().zip(lookups) {
            let stage_start = Instant::now();
            let base = self.classify_finalization_raw_lookup(lookup);
            self.mutation_stats.deferred_finalization_parse_us = self
                .mutation_stats
                .deferred_finalization_parse_us
                .saturating_add(elapsed_us(stage_start));
            let base = base?;

            let stage_start = Instant::now();
            let state = self.replay_deferred_operations(entry, base);
            self.mutation_stats.deferred_finalization_replay_us = self
                .mutation_stats
                .deferred_finalization_replay_us
                .saturating_add(elapsed_us(stage_start));
            let state = state?;

            let stage_start = Instant::now();
            let value = state.map(DeferredNodeState::into_bytes).transpose();
            self.mutation_stats.deferred_finalization_encode_us = self
                .mutation_stats
                .deferred_finalization_encode_us
                .saturating_add(elapsed_us(stage_start));
            let value = value?;
            overlay.push((Self::key_for(self.prefix, &hash), value));
        }
        Ok(overlay)
    }

    fn promote_deferred_entry(&mut self, hash: &UInt256) -> MptResult<()> {
        let Some(entry) = self.deferred_entries.get(hash).copied() else {
            return Ok(());
        };
        let key = node_key_bytes(self.prefix, hash);
        let lookup_started = Instant::now();
        let lookup = match self.store.try_get_node_with_source(&key) {
            Ok(lookup) => lookup,
            Err(error) => {
                self.mutation_stats.trie_resolve_store_ns = self
                    .mutation_stats
                    .trie_resolve_store_ns
                    .saturating_add(elapsed_ns(lookup_started));
                self.mutation_stats.finalization_lookup_errors = self
                    .mutation_stats
                    .finalization_lookup_errors
                    .saturating_add(1);
                return Err(error);
            }
        };
        let base = self.classify_finalization_lookup(*hash, lookup);
        self.mutation_stats.trie_resolve_store_ns = self
            .mutation_stats
            .trie_resolve_store_ns
            .saturating_add(elapsed_ns(lookup_started));
        if base.is_some() {
            self.mutation_stats.trie_resolve_store_hits = self
                .mutation_stats
                .trie_resolve_store_hits
                .saturating_add(1);
        } else {
            self.mutation_stats.trie_resolve_store_misses = self
                .mutation_stats
                .trie_resolve_store_misses
                .saturating_add(1);
        }
        let base_present = base.is_some();
        let base = base
            .map(|node| DeferredNodeState::from_node(&node))
            .transpose()?;
        let state = self.replay_deferred_operations(entry, base)?;
        let trackable = match state {
            Some(state) => {
                let node_type = state.node_type;
                let reference = state.reference;
                let payload_without_reference = state.payload_without_reference.clone();
                let node = state.into_node(*hash)?;
                MptTrackable {
                    node: Some(node),
                    node_type: Some(node_type),
                    reference,
                    payload_without_reference: Some(payload_without_reference),
                    state: if base_present {
                        TrackState::Changed
                    } else {
                        TrackState::Added
                    },
                    produced_in_current_commit: true,
                }
            }
            None => MptTrackable {
                node: None,
                node_type: None,
                reference: 0,
                payload_without_reference: None,
                state: if base_present {
                    TrackState::Deleted
                } else {
                    TrackState::None
                },
                produced_in_current_commit: true,
            },
        };
        self.deferred_entries.remove(hash);
        self.entries.insert(*hash, trackable);
        Ok(())
    }

    fn resolve_internal(&mut self, hash: &UInt256) -> MptResult<&mut MptTrackable> {
        if self.deferred_entries.contains_key(hash) {
            self.promote_deferred_entry(hash)?;
            return self.entries.get_mut(hash).ok_or_else(|| {
                MptError::invalid("promoted deferred MPT entry is missing from the cache")
            });
        }
        let store = self.store.as_ref();
        let prefix = self.prefix;
        let stats = &mut self.mutation_stats;

        match self.entries.entry(*hash) {
            Entry::Occupied(entry) => {
                stats.trie_resolve_cache_hits = stats.trie_resolve_cache_hits.saturating_add(1);
                Ok(entry.into_mut())
            }
            Entry::Vacant(entry) => {
                let lookup_started = Instant::now();
                let node = match Self::load_from_store_snapshot(store, prefix, hash) {
                    Ok(node) => node,
                    Err(error) => {
                        stats.trie_resolve_store_ns = stats
                            .trie_resolve_store_ns
                            .saturating_add(elapsed_ns(lookup_started));
                        return Err(error);
                    }
                };
                stats.trie_resolve_store_ns = stats
                    .trie_resolve_store_ns
                    .saturating_add(elapsed_ns(lookup_started));
                if node.is_some() {
                    stats.trie_resolve_store_hits = stats.trie_resolve_store_hits.saturating_add(1);
                } else {
                    stats.trie_resolve_store_misses =
                        stats.trie_resolve_store_misses.saturating_add(1);
                }
                Ok(entry.insert(MptTrackable::new(node)))
            }
        }
    }

    fn resolve_finalized(&mut self, hash: &UInt256) -> MptResult<&mut MptTrackable> {
        let store = self.store.as_ref();
        let prefix = self.prefix;
        let stats = &mut self.mutation_stats;

        match self.entries.entry(*hash) {
            Entry::Occupied(entry) => {
                stats.finalization_cache_hits = stats.finalization_cache_hits.saturating_add(1);
                Ok(entry.into_mut())
            }
            Entry::Vacant(entry) => {
                let key = node_key_bytes(prefix, hash);
                let lookup = match store.try_get_node_with_source(&key) {
                    Ok(lookup) => lookup,
                    Err(error) => {
                        stats.finalization_lookup_errors =
                            stats.finalization_lookup_errors.saturating_add(1);
                        return Err(error);
                    }
                };
                let mut node = match lookup {
                    MptStoreLookup::InMemory(node) => {
                        if node.is_some() {
                            stats.finalization_memory_hits =
                                stats.finalization_memory_hits.saturating_add(1);
                        } else {
                            stats.finalization_memory_misses =
                                stats.finalization_memory_misses.saturating_add(1);
                        }
                        node
                    }
                    MptStoreLookup::Backing(node) => {
                        if node.is_some() {
                            stats.finalization_backing_hits =
                                stats.finalization_backing_hits.saturating_add(1);
                        } else {
                            stats.finalization_backing_misses =
                                stats.finalization_backing_misses.saturating_add(1);
                        }
                        node
                    }
                };
                if let Some(node) = node.as_mut() {
                    node.set_accounted_hash(*hash);
                }
                Ok(entry.insert(MptTrackable::new(node)))
            }
        }
    }

    fn load_from_store_snapshot(store: &S, prefix: u8, hash: &UInt256) -> MptResult<Option<Node>> {
        let key = node_key_bytes(prefix, hash);
        let mut node = store.try_get_node(&key)?;
        if let Some(node) = node.as_mut() {
            node.set_accounted_hash(*hash);
        }
        Ok(node)
    }

    fn key(&self, hash: &UInt256) -> Vec<u8> {
        Self::key_for(self.prefix, hash)
    }

    fn key_for(prefix: u8, hash: &UInt256) -> Vec<u8> {
        node_key_bytes(prefix, hash).to_vec()
    }
}
