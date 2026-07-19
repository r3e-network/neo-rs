use super::*;

impl PackStore {
    /// Runs derived index maintenance after no prepared append remains.
    ///
    /// Coordinated callers invoke this only after the external marker commits
    /// and the prepared frame becomes visible. A maintenance failure cannot
    /// roll back that committed frame; callers may drop the writer and let
    /// startup recovery rebuild the derived indexes from the marker horizon.
    pub fn maintain(&mut self) -> Result<()> {
        ensure!(
            self.pending_append.is_none(),
            "cannot maintain index runs while an append awaits activation"
        );
        while let Some(plan) = self.plan_compaction()? {
            self.adopt_compaction(plan.build()?)?;
        }
        Ok(())
    }

    /// Reports bounded derived-index debt without performing I/O.
    pub fn compaction_debt(&self) -> CompactionDebt {
        let mut excess_runs = 0usize;
        let mut backpressure_required = false;
        for (&level, &count) in &self.level_run_counts {
            let bound = self.level_run_bound(level);
            excess_runs = excess_runs.saturating_add(count.saturating_sub(bound));
            backpressure_required |= count >= bound.saturating_add(self.compaction.fanout);
        }
        CompactionDebt {
            live_runs: u64::try_from(self.runs.len()).unwrap_or(u64::MAX),
            excess_runs: u64::try_from(excess_runs).unwrap_or(u64::MAX),
            decoded_index_bytes: self.decoded_index_bytes,
            max_index_memory_bytes: self.max_index_memory_bytes,
            backpressure_required,
        }
    }

    /// Selects and leases the oldest inputs of the first overfull level.
    /// Selection and snapshot pinning are short; [`PackCompactionPlan::build`]
    /// may then run on another thread without holding the pack writer lock.
    pub fn plan_compaction(&self) -> Result<Option<PackCompactionPlan>> {
        let Some(level) = self.first_overfull_level() else {
            return Ok(None);
        };
        self.plan_compaction_at_level(level)
    }

    /// Adopts a previously built derived run into the latest live view and
    /// durably publishes a new manifest. Appends that landed while the output
    /// was built remain in the manifest; only the exact leased inputs leave.
    pub fn adopt_compaction(&mut self, prepared: PreparedPackCompaction) -> Result<()> {
        ensure!(
            self.pending_append.is_none(),
            "cannot adopt compaction while an append awaits activation"
        );
        let adoption_started = Instant::now();
        let PreparedPackCompaction {
            pending,
            _source_snapshot,
        } = prepared;
        ensure!(
            pending.inputs.iter().all(|input| self
                .runs
                .iter()
                .any(|current| same_live_run(current, input))),
            "compaction inputs are no longer part of the live generation"
        );
        let decoded_index_bytes = self
            .decoded_index_bytes
            .checked_sub(pending.input_memory_bytes)
            .and_then(|bytes| bytes.checked_add(pending.run.memory_bytes))
            .context("decoded index bytes overflow")?;
        ensure!(
            decoded_index_bytes <= self.max_index_memory_bytes,
            "compaction output exceeds configured index memory bound"
        );
        let file_bytes = pending.run.file_bytes;
        let mut candidate_runs = self.runs.clone();
        candidate_runs.retain(|current| {
            !pending
                .inputs
                .iter()
                .any(|input| same_live_run(current, input))
        });
        candidate_runs.push(LiveRun {
            run: Arc::new(pending.run),
            level: pending.level,
            min_epoch: pending.min_epoch,
            max_epoch: pending.max_epoch,
        });
        candidate_runs.sort_by_key(|live| live.min_epoch);
        let candidate_level_run_counts = count_run_levels(&candidate_runs);
        let candidate_ranges = run_ranges(&candidate_runs);
        let generation = self
            .generation
            .checked_add(1)
            .context("manifest generation overflows")?;
        let candidate_manifest = Manifest {
            generation,
            entries: candidate_runs.iter().map(manifest_entry_of).collect(),
        };
        manifest::publish_manifest(&self.root, &candidate_manifest)?;

        // Everything after durable manifest publication is an infallible
        // in-process view installation.
        self.runs = candidate_runs;
        self.level_run_counts = candidate_level_run_counts;
        self.ranges = candidate_ranges;
        self.decoded_index_bytes = decoded_index_bytes;
        self.generation = generation;
        self.stats.cycles = self.stats.cycles.saturating_add(1);
        self.stats.runs_merged = self.stats.runs_merged.saturating_add(pending.input_runs);
        self.stats.runs_produced = self.stats.runs_produced.saturating_add(1);
        self.stats.input_records = self
            .stats
            .input_records
            .saturating_add(pending.input_records);
        self.stats.output_records = self
            .stats
            .output_records
            .saturating_add(pending.output_records);
        self.stats.bytes_written = self.stats.bytes_written.saturating_add(file_bytes);
        self.stats.wall_ns = self.stats.wall_ns.saturating_add(
            pending
                .wall_ns
                .saturating_add(duration_ns(adoption_started.elapsed())),
        );
        self.note_peak();
        Ok(())
    }

    /// Atomically republishes the unchanged live run set in the current
    /// manifest format.
    ///
    /// Offline migration tooling uses this after fully validating a legacy
    /// payload prefix and before publishing a new external identity. No frame,
    /// index record, or read-visible value changes.
    pub fn republish_manifest(&mut self) -> Result<()> {
        ensure!(
            self.pending_append.is_none(),
            "cannot republish a manifest while an append awaits activation"
        );
        ensure!(
            self.last_frame_receipt.is_some(),
            "cannot republish a manifest for an empty pack"
        );
        self.publish_manifest()
    }

    /// Newest-committed-version point read.
    pub fn get(&self, key: &[u8; PACK_KEY_BYTES]) -> Result<Option<Vec<u8>>> {
        self.view().get(key)
    }

    /// Filter-assisted k-way merge: per-run cursors gallop forward over the
    /// sparse fences as the sorted query stream advances, so each run is
    /// visited once per batch instead of once per key binary search.
    /// Filter-assisted k-way batch read. Keys must be sorted ascending;
    /// results align one-to-one with the input order.
    pub fn get_many_sorted(&self, keys: &[[u8; PACK_KEY_BYTES]]) -> Result<Vec<Option<Vec<u8>>>> {
        self.view().get_many_sorted(keys)
    }

    fn view(&self) -> ReadView<'_> {
        ReadView {
            runs: &self.runs,
            ranges: &self.ranges,
            pack_map: &self.pack_map,
            lookup_pack_map: self.lookup_pack_map.as_deref(),
        }
    }

    /// Publishes the current live run set as the next manifest generation.
    /// The rename inside `manifest::publish_manifest` is the single atomic
    /// publication point; a crash before it leaves the previous generation
    /// live and orphans only unreferenced run files.
    fn publish_manifest(&mut self) -> Result<()> {
        let generation = self
            .generation
            .checked_add(1)
            .context("manifest generation overflows")?;
        let entries = self.runs.iter().map(manifest_entry_of).collect();
        manifest::publish_manifest(
            &self.root,
            &Manifest {
                generation,
                entries,
            },
        )?;
        self.generation = generation;
        Ok(())
    }

    fn level_run_bound(&self, level: u32) -> usize {
        if level == 0 {
            self.compaction.l0_bound
        } else {
            self.compaction.l1_bound
        }
    }

    fn first_overfull_level(&self) -> Option<u32> {
        self.level_run_counts
            .iter()
            .find_map(|(&level, &count)| (count > self.level_run_bound(level)).then_some(level))
    }

    fn plan_compaction_at_level(&self, level: u32) -> Result<Option<PackCompactionPlan>> {
        ensure!(
            level < MAX_COMPACTION_LEVEL,
            "derived index exceeded the maximum compaction level"
        );
        let source_snapshot = self.snapshot()?;
        let inputs: Vec<LiveRun> = source_snapshot
            .runs
            .iter()
            .filter(|live| live.level == level)
            .take(self.compaction.fanout)
            .cloned()
            .collect();
        if inputs.len() < 2 {
            return Ok(None);
        }
        let estimated_workspace_bytes =
            estimate_compaction_workspace_for_inputs(&inputs, self.decoded_index_bytes);
        Ok(Some(PackCompactionPlan {
            level,
            inputs,
            runs_dir: self.runs_dir.clone(),
            random_point_mmap: self.options.random_point_mmap,
            estimated_workspace_bytes,
            resident_index_bytes: self.decoded_index_bytes,
            max_index_memory_bytes: self.max_index_memory_bytes,
            _source_snapshot: source_snapshot,
        }))
    }

    /// Merges the oldest runs of one level (up to the fanout) into one run
    /// at the next level: records are decoded, checksum-scrubbed, merged
    /// newest-epoch-wins, and re-encoded with rebuilt fences and filter.
    /// The output is an ordinary v3 run file whose payload offsets keep
    /// pointing at the original frames. Nothing in the live set changes and
    /// no manifest is published here, so calling this without adopting the
    /// result exactly simulates a crash after run-file publication.
    #[cfg(test)]
    pub(super) fn build_compacted_run(&self, level: u32) -> Result<Option<PendingMerge>> {
        let Some(plan) = self.plan_compaction_at_level(level)? else {
            return Ok(None);
        };
        Ok(Some(plan.build()?.pending))
    }

    /// Pins the current manifest generation: the snapshot keeps its own run
    /// references and pack mapping, and the lease blocks reclamation of the
    /// generation's run files until the snapshot is dropped.
    pub fn snapshot(&self) -> Result<Snapshot> {
        self.pin_snapshot_parts(
            self.generation,
            &self.runs,
            &self.ranges,
            &self.pack_map,
            self.lookup_pack_map.as_ref(),
        )
    }

    /// Pins an immutable generation assembled either from the current view or
    /// by pre-seal validation before its manifest publication.
    pub(super) fn pin_snapshot_parts(
        &self,
        generation: u64,
        runs: &[LiveRun],
        ranges: &[RunRange],
        pack_map: &Arc<Mmap>,
        lookup_pack_map: Option<&Arc<Mmap>>,
    ) -> Result<Snapshot> {
        {
            let mut leases = self
                .leases
                .lock()
                .map_err(|error| anyhow::anyhow!("snapshot lease book is poisoned: {error}"))?;
            *leases.entry(generation).or_insert(0) += 1;
        }
        Ok(Snapshot {
            generation,
            runs: runs.to_vec(),
            ranges: ranges.to_vec(),
            pack_map: Arc::clone(pack_map),
            lookup_pack_map: lookup_pack_map.map(Arc::clone),
            leases: Arc::clone(&self.leases),
        })
    }

    /// Explicit reclamation of superseded runs and manifests. The current
    /// generation and every leased generation stay untouched; everything
    /// else (superseded manifests, their run files, orphan runs from
    /// interrupted appends or compactions, stale temp files) is deleted.
    /// Never called implicitly during reads.
    pub fn gc(&mut self) -> Result<GcStats> {
        ensure!(
            self.pending_append.is_none(),
            "cannot reclaim files while an append awaits activation"
        );
        let mut protected_generations: Vec<u64> = {
            let leases = self
                .leases
                .lock()
                .map_err(|error| anyhow::anyhow!("snapshot lease book is poisoned: {error}"))?;
            leases.keys().copied().collect()
        };
        if self.generation > 0 && !protected_generations.contains(&self.generation) {
            protected_generations.push(self.generation);
        }
        let mut protected_runs: HashSet<String> = HashSet::new();
        for generation in &protected_generations {
            let path = self.root.join(manifest::manifest_file_name(*generation));
            let manifest = manifest::read_manifest(&path)
                .with_context(|| format!("load protected manifest generation {generation}"))?;
            protected_runs.extend(manifest.entries.into_iter().map(|entry| entry.file_name));
        }
        let mut stats = GcStats::default();
        for entry in fs::read_dir(&self.runs_dir)
            .with_context(|| format!("read index-run directory {}", self.runs_dir.display()))?
        {
            let entry = entry.context("read index-run directory entry")?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or_default();
            let delete = match extension {
                // A compaction plan builds outside the writer lock. Runtime
                // GC therefore leaves temp files alone; startup recovery owns
                // stale-temp cleanup after acquiring the exclusive lease.
                "tmp" => false,
                "idx" => !protected_runs.contains(name),
                _ => false,
            };
            if !delete {
                continue;
            }
            let bytes = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
            fs::remove_file(&path)
                .with_context(|| format!("delete superseded index run {}", path.display()))?;
            stats.bytes_reclaimed = stats.bytes_reclaimed.saturating_add(bytes);
            if extension == "idx" {
                stats.runs_deleted = stats.runs_deleted.saturating_add(1);
            }
        }
        for (generation, path) in manifest::list_manifest_files(&self.root)? {
            if protected_generations.contains(&generation) {
                continue;
            }
            let bytes = fs::metadata(&path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            fs::remove_file(&path)
                .with_context(|| format!("delete superseded manifest {}", path.display()))?;
            stats.manifests_deleted = stats.manifests_deleted.saturating_add(1);
            stats.bytes_reclaimed = stats.bytes_reclaimed.saturating_add(bytes);
        }
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("read pack store directory {}", self.root.display()))?
        {
            let entry = entry.context("read pack store directory entry")?;
            let path = entry.path();
            let is_manifest_tmp = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("manifest-") && name.ends_with(".tmp"));
            if is_manifest_tmp {
                fs::remove_file(&path)
                    .with_context(|| format!("delete stale manifest temp {}", path.display()))?;
            }
        }
        sync_directory(&self.runs_dir)?;
        sync_directory(&self.root)?;
        self.stats.gc_cycles = self.stats.gc_cycles.saturating_add(1);
        self.stats.gc_runs_deleted = self
            .stats
            .gc_runs_deleted
            .saturating_add(stats.runs_deleted);
        self.stats.gc_manifests_deleted = self
            .stats
            .gc_manifests_deleted
            .saturating_add(stats.manifests_deleted);
        self.stats.gc_bytes_reclaimed = self
            .stats
            .gc_bytes_reclaimed
            .saturating_add(stats.bytes_reclaimed);
        Ok(stats)
    }

    /// Physical layout: (pack bytes, live index bytes, live run count,
    /// decoded index memory bytes).
    pub fn layout(&self) -> Result<(u64, u64, u64, u64)> {
        let pack_bytes = self.pack.metadata().context("stat append pack")?.len();
        let index_bytes = self.runs.iter().try_fold(0u64, |total, live| {
            total
                .checked_add(live.run.file_bytes)
                .context("index bytes overflow")
        })?;
        Ok((
            pack_bytes,
            index_bytes,
            u64::try_from(self.runs.len()).context("run count does not fit u64")?,
            self.decoded_index_bytes,
        ))
    }

    /// Structural counts observed when this handle opened the store.
    pub const fn open_validation(&self) -> OpenValidation {
        self.open_validation
    }

    /// Cumulative compaction and reclamation evidence for this store.
    pub const fn compaction_stats(&self) -> CompactionStats {
        self.stats
    }

    /// Placement and checksum of the newest visible frame. A prepared frame
    /// does not replace this receipt until activation publishes its manifest.
    pub const fn last_frame_receipt(&self) -> Option<PackFrameReceipt> {
        self.last_frame_receipt
    }

    pub(super) fn note_peak(&mut self) {
        debug_assert_eq!(
            self.level_run_counts.values().sum::<usize>(),
            self.runs.len(),
            "per-level run directory diverged from the live manifest"
        );
        self.stats.peak_live_runs = self
            .stats
            .peak_live_runs
            .max(u64::try_from(self.runs.len()).unwrap_or(u64::MAX));
    }
}
