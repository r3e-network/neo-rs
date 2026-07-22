use super::segment::{
    SEGMENT_HEADER_LEN, SegmentCatalogEntry, create_initial_segment, discover_segment_catalog,
    open_segment_for_append, open_segment_read_only, required_segment_prefix,
};
use super::*;

/// One read-only, structurally scanned segment discovered before recovery is
/// allowed to mutate the store directory.
struct ScannedSegment {
    id: PackSegmentId,
    path: PathBuf,
    first_epoch: u64,
    complete_frames: u64,
    complete_end: u64,
    file_bytes: u64,
}

/// Read-only recovery catalog. Segment scans retain only fixed-size summaries;
/// every file descriptor is closed before the next segment is opened.
struct ScannedCatalog {
    entries: Vec<SegmentCatalogEntry>,
    required: Vec<ScannedSegment>,
    orphan_complete_frames: u64,
}

/// Exact authenticated frame history selected by a manifest or external
/// commit marker. Frame boundaries are streamed again when they are needed.
struct SelectedSegment {
    id: PackSegmentId,
    path: PathBuf,
    first_epoch: u64,
    frame_count: u64,
    committed_end: u64,
}

impl SelectedSegment {
    fn committed_end(&self) -> u64 {
        self.committed_end
    }
}

struct SegmentSelection {
    segments: Vec<SelectedSegment>,
    extents: Vec<ManifestExtent>,
    frame_count: u64,
    last_frame_receipt: Option<PackFrameReceipt>,
}

/// Conservative immutable-run shape produced by deterministic recovery.
/// Record counts are upper bounds until the corresponding merge is built,
/// because newer records can supersede older keys during compaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PlannedRun {
    level: u32,
    min_epoch: u64,
    max_epoch: u64,
    record_count: u64,
    memory_bytes: u64,
}

/// Complete, read-only proof that recovery can finish within configured
/// manifest, level, and memory bounds before touching any durable artifact.
#[derive(Debug)]
struct RebuildPlan {
    runs: Vec<PlannedRun>,
    peak_index_bytes: u64,
}

struct RebuildPlanner {
    runs: Vec<PlannedRun>,
    resident_bytes: u64,
    peak_index_bytes: u64,
    expected_epoch: u64,
    config: PackStoreConfig,
}

impl RebuildPlanner {
    fn new(config: PackStoreConfig) -> Self {
        Self {
            runs: Vec::new(),
            resident_bytes: 0,
            peak_index_bytes: 0,
            expected_epoch: 0,
            config,
        }
    }

    fn push_frame(
        &mut self,
        epoch: u64,
        metadata_bytes: u64,
        rows: u64,
        distinct_keys: usize,
    ) -> Result<()> {
        ensure!(
            epoch == self.expected_epoch,
            "recovery frame epochs are not contiguous from zero"
        );
        let decode_floor =
            rebuild_decode_workspace_bytes(self.resident_bytes, metadata_bytes, rows)?;
        self.charge_peak(decode_floor)?;
        let frame_peak =
            estimate_rebuild_peak_bytes(self.resident_bytes, metadata_bytes, rows, distinct_keys)?;
        self.charge_peak(frame_peak)?;

        let memory_bytes = manifest_run_memory_bytes(rows)?;
        self.resident_bytes = self
            .resident_bytes
            .checked_add(memory_bytes)
            .context("recovery resident index bytes overflow")?;
        self.charge_peak(self.resident_bytes)?;
        self.runs.push(PlannedRun {
            level: 0,
            min_epoch: epoch,
            max_epoch: epoch,
            record_count: rows,
            memory_bytes,
        });
        self.expected_epoch = self
            .expected_epoch
            .checked_add(1)
            .context("recovery planned epoch overflows")?;

        while self.runs.len() >= 2 {
            let right = self.runs[self.runs.len() - 1];
            let left = self.runs[self.runs.len() - 2];
            if left.level != right.level {
                break;
            }
            self.merge_tail(left, right)?;
        }
        Ok(())
    }

    fn merge_tail(&mut self, left: PlannedRun, right: PlannedRun) -> Result<()> {
        ensure!(
            left.max_epoch.checked_add(1) == Some(right.min_epoch),
            "recovery carry inputs are not contiguous"
        );
        let level = left
            .level
            .checked_add(1)
            .context("recovery carry level overflows")?;
        let level_count = u64::from(level)
            .checked_add(1)
            .context("recovery level count overflows")?;
        let maximum_levels = u64::from(self.config.max_index_levels());
        if level_count > maximum_levels {
            return Err(PackStoreError::LimitExceeded {
                limit: PackStoreLimit::IndexLevels,
                actual: level_count,
                maximum: maximum_levels,
            }
            .into());
        }

        let record_count = left
            .record_count
            .checked_add(right.record_count)
            .context("recovery merge record upper bound overflows")?;
        let memory_bytes = manifest_run_memory_bytes(record_count)?;
        self.charge_peak(estimate_compaction_workspace(
            self.resident_bytes,
            record_count,
        ))?;
        self.charge_peak(
            self.resident_bytes
                .checked_add(memory_bytes)
                .context("recovery merge overlap bytes overflow")?,
        )?;

        self.resident_bytes = self
            .resident_bytes
            .checked_sub(left.memory_bytes)
            .and_then(|bytes| bytes.checked_sub(right.memory_bytes))
            .and_then(|bytes| bytes.checked_add(memory_bytes))
            .context("recovery resident bytes changed outside the carry plan")?;
        self.runs.truncate(self.runs.len() - 2);
        self.runs.push(PlannedRun {
            level,
            min_epoch: left.min_epoch,
            max_epoch: right.max_epoch,
            record_count,
            memory_bytes,
        });
        Ok(())
    }

    fn charge_peak(&mut self, bytes: u64) -> Result<()> {
        ensure_rebuild_memory_bound(bytes, self.config.max_index_memory_bytes())?;
        self.peak_index_bytes = self.peak_index_bytes.max(bytes);
        Ok(())
    }

    fn finish(self) -> Result<RebuildPlan> {
        let run_count =
            u64::try_from(self.runs.len()).context("recovery final run count does not fit u64")?;
        let configured_runs = u64::try_from(self.config.max_recent_runs())
            .context("configured recent-run limit does not fit u64")?;
        let maximum_runs = configured_runs.min(PackStoreConfig::HARD_MAX_RECENT_RUNS as u64);
        if run_count > maximum_runs {
            return Err(PackStoreError::LimitExceeded {
                limit: PackStoreLimit::RecentRuns,
                actual: run_count,
                maximum: maximum_runs,
            }
            .into());
        }
        for pair in self.runs.windows(2) {
            ensure!(
                pair[0].max_epoch.checked_add(1) == Some(pair[1].min_epoch),
                "recovery final run ranges are not contiguous"
            );
        }
        Ok(RebuildPlan {
            runs: self.runs,
            peak_index_bytes: self.peak_index_bytes,
        })
    }
}

#[cfg(test)]
pub(super) fn plan_uniform_rebuild_for_test(
    config: PackStoreConfig,
    frame_count: u64,
    metadata_bytes: u64,
    rows: u64,
    distinct_keys: usize,
) -> Result<(u64, Vec<(u32, u64, u64)>)> {
    let mut planner = RebuildPlanner::new(config);
    for epoch in 0..frame_count {
        planner.push_frame(epoch, metadata_bytes, rows, distinct_keys)?;
    }
    let plan = planner.finish()?;
    Ok((
        plan.peak_index_bytes,
        plan.runs
            .iter()
            .map(|run| (run.level, run.min_epoch, run.max_epoch))
            .collect(),
    ))
}

impl SegmentSelection {
    fn tip_id(&self) -> PackSegmentId {
        self.segments
            .last()
            .expect("authenticated selection contains segment zero")
            .id
    }

    fn tip_end(&self) -> u64 {
        self.segments
            .last()
            .expect("authenticated selection contains segment zero")
            .committed_end()
    }
}

impl PackStore {
    /// Creates an empty pack store in `root` using one validated resource
    /// contract. The directory must be missing, empty, or contain only exact
    /// unpublished artifacts from an interrupted initial creation. Once the
    /// canonical segment-zero name exists, the store is opened rather than
    /// created again.
    pub fn create(root: &Path, config: PackStoreConfig) -> PackStoreResult<Self> {
        Self::create_inner(root, config)
            .map_err(|error| PackStoreError::classify_create(error, root))
    }

    fn create_inner(root: &Path, config: PackStoreConfig) -> Result<Self> {
        config.validate()?;
        let options = config.read_options().normalized_for_host();
        let config = config.with_read_options(options)?;
        read_view::preflight_pack_value_pool(options.batch_value_workers)?;
        let root_existed = root.exists();
        if root_existed {
            preflight_store_creation(root)?;
        } else {
            fs::create_dir_all(root)
                .with_context(|| format!("create pack store directory {}", root.display()))?;
            sync_parent_directory(root)?;
        }
        let writer_lease = acquire_writer_lease(root)?;
        clear_interrupted_store_creation(root)?;
        let runs_dir = root.join("runs");
        fs::create_dir(&runs_dir)
            .with_context(|| format!("create index-run directory {}", runs_dir.display()))?;
        let (pack, pack_path) = create_initial_segment(root)?;
        let segments = Arc::new(SegmentSet::open(
            root,
            &[],
            options,
            config.max_segment_bytes(),
        )?);
        Ok(Self {
            root: root.to_path_buf(),
            runs_dir,
            pack,
            pack_path,
            active_segment_id: PackSegmentId::INITIAL,
            segments,
            runs: Vec::new(),
            level_run_counts: BTreeMap::new(),
            ranges: Vec::new(),
            next_epoch: 0,
            generation: 0,
            extents: Vec::new(),
            decoded_index_bytes: 0,
            config,
            stats: CompactionStats::default(),
            stage_totals: PackStageTotals::default(),
            logical_payload_bytes: 0,
            rebuild: RebuildMetrics::default(),
            read_counters: Arc::new(ReadCounters::default()),
            leases: Arc::new(Mutex::new(BTreeMap::new())),
            open_validation: OpenValidation {
                frames: 0,
                runs: 0,
                index_entries: 0,
            },
            last_frame_receipt: None,
            pending_append: None,
            instance_id: next_store_instance_id(),
            next_prepare_serial: 0,
            _writer_lease: writer_lease,
            inflight_compaction_outputs: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    /// Opens a store through the newest manifest generation with complete
    /// committed-frame and index-record authentication. Missing or corrupt
    /// derived indexes are rebuilt from committed frames (a slow but correct
    /// recovery path); a manifest ahead of the validated frame chain is fatal.
    pub fn open(root: &Path, config: PackStoreConfig) -> PackStoreResult<Self> {
        Self::open_inner(root, config).map_err(|error| PackStoreError::classify_open(error, root))
    }

    /// Opens a pack at the exact horizon selected by an external durable
    /// commit marker.
    ///
    /// A missing horizon means no frame is canonical. Complete frames or
    /// manifests beyond the horizon are orphan suffixes: the pack is
    /// truncated and all derived indexes are rebuilt from the retained
    /// prefix. A marker that names absent or checksum-mismatched bytes fails
    /// closed.
    pub fn open_at_commit_horizon(
        root: &Path,
        config: PackStoreConfig,
        horizon: Option<PackCommitHorizon>,
    ) -> PackStoreResult<Self> {
        Self::open_at_commit_horizon_inner(root, config, horizon)
            .map_err(|error| PackStoreError::classify_open(error, root))
    }

    fn open_at_commit_horizon_inner(
        root: &Path,
        config: PackStoreConfig,
        horizon: Option<PackCommitHorizon>,
    ) -> Result<Self> {
        config.validate()?;
        let options = config.read_options().normalized_for_host();
        let config = config.with_read_options(options)?;
        read_view::preflight_pack_value_pool(options.batch_value_workers)?;
        let writer_lease = acquire_writer_lease(root)?;
        let required_tip = horizon.map_or(PackSegmentId::INITIAL, |horizon| horizon.segment_id);
        let catalog = scan_segment_catalog(root, required_tip, config)?;
        let expected_frames = match horizon {
            Some(horizon) => horizon
                .epoch
                .checked_add(1)
                .context("committed pack epoch overflows")?,
            None => 0,
        };
        let selection = match horizon {
            Some(horizon) => select_commit_horizon(&catalog.required, horizon, config)?,
            None => select_empty_history(&catalog.required)?,
        };
        ensure!(
            selection.frame_count == expected_frames,
            "authenticated pack history exposes {} frames, expected {expected_frames}",
            selection.frame_count
        );
        let authenticated_extents = selection.extents.clone();
        let newest_manifest = manifest::newest_manifest_file(root)?;
        let current_manifest = match newest_manifest.as_ref() {
            Some((_, path)) => match manifest::read_manifest(path) {
                Ok(manifest) => Some(manifest),
                Err(error) if is_unsupported_artifact_version(&error) => return Err(error),
                Err(_) => None,
            },
            None => None,
        };
        let manifest_frames = current_manifest
            .as_ref()
            .and_then(|manifest| manifest.frame_count().ok());
        let manifest_binding_valid = current_manifest
            .as_ref()
            .is_some_and(|manifest| manifest.extents == authenticated_extents);
        let manifest_runs_valid =
            if manifest_frames == Some(expected_frames) && manifest_binding_valid {
                match Self::load_manifest_runs(
                    &root.join("runs"),
                    current_manifest
                        .as_ref()
                        .expect("validated manifest presence"),
                    config,
                    options,
                ) {
                    Ok(_) => true,
                    Err(error) if is_non_rebuildable_run_error(&error) => return Err(error),
                    Err(_) => false,
                }
            } else {
                false
            };
        let fast_open = if expected_frames == 0 {
            catalog
                .required
                .iter()
                .all(|segment| segment.complete_frames == 0)
                && catalog.orphan_complete_frames == 0
                && newest_manifest.is_none()
                && fs::read_dir(root.join("runs"))
                    .context("read index-run directory for empty horizon")?
                    .next()
                    .is_none()
        } else {
            manifest_runs_valid
        };
        let rebuild_generation = if !fast_open && expected_frames > 0 {
            Some(next_rebuilt_manifest_generation(
                newest_manifest.as_ref().map(|(generation, _)| *generation),
            )?)
        } else {
            None
        };
        let rebuild_plan = if !fast_open && expected_frames > 0 {
            Some(Self::preflight_rebuild_runs_from_segments(
                &selection.segments,
                config,
            )?)
        } else {
            None
        };

        // All selected bytes, configured resource bounds, and any required
        // rebuild allocation have been proved above. Only now may recovery
        // truncate the selected tip or remove later orphan segments.
        reconcile_segment_files(root, &catalog.entries, &selection)?;
        if fast_open {
            clear_stale_temp_files(root)?;
            clear_rebuild_staging(&root.join("runs"))?;
        } else if expected_frames == 0 {
            clear_derived_visibility(root)?;
        } else {
            clear_stale_temp_files(root)?;
            let loaded = Self::rebuild_runs_from_segments(
                &selection.segments,
                &root.join("runs"),
                options,
                config,
                rebuild_plan
                    .as_ref()
                    .expect("non-empty slow recovery has a preflight plan"),
            )?;
            publish_rebuilt_manifest(
                root,
                rebuild_generation.expect("non-empty slow recovery has a generation"),
                &authenticated_extents,
                &loaded,
            )?;
        }

        let mut store = Self::open_with_lease(root, config, writer_lease)?;
        ensure!(
            store.open_validation.frames == expected_frames,
            "recovered pack exposes {} frames, expected {expected_frames}",
            store.open_validation.frames
        );
        match (horizon, store.last_frame_receipt) {
            (Some(horizon), Some(receipt)) => ensure!(
                receipt.epoch == horizon.epoch
                    && receipt.segment_id == horizon.segment_id
                    && receipt.frame_end == horizon.frame_end
                    && receipt.context == horizon.context
                    && receipt.frame_sha256 == horizon.frame_sha256,
                "recovered pack tail does not match the canonical commit marker"
            ),
            (Some(_), None) => anyhow::bail!("recovered pack has no committed tail frame"),
            (None, Some(_)) => anyhow::bail!("uncommitted pack frames remain visible"),
            (None, None) => {}
        }
        store.gc()?;
        Ok(store)
    }

    fn open_inner(root: &Path, config: PackStoreConfig) -> Result<Self> {
        config.validate()?;
        let options = config.read_options().normalized_for_host();
        let config = config.with_read_options(options)?;
        read_view::preflight_pack_value_pool(options.batch_value_workers)?;
        let writer_lease = acquire_writer_lease(root)?;
        let mut store = Self::open_with_lease(root, config, writer_lease)?;
        store.gc()?;
        Ok(store)
    }

    fn open_with_lease(root: &Path, config: PackStoreConfig, writer_lease: File) -> Result<Self> {
        config.validate()?;
        let options = config.read_options();
        let runs_dir = root.join("runs");
        let newest_manifest = manifest::newest_manifest_file(root)?;
        let current_manifest = match newest_manifest.as_ref() {
            Some((_, path)) => Some(manifest::read_manifest(path).with_context(|| {
                format!(
                    "read newest manifest {}; visibility authority is unavailable",
                    path.display()
                )
            })?),
            None => None,
        };
        let required_tip = current_manifest
            .as_ref()
            .and_then(|manifest| manifest.extents.last())
            .map_or(PackSegmentId::INITIAL, |extent| extent.segment_id);
        let catalog = scan_segment_catalog(root, required_tip, config)?;
        let mut generation = 0u64;
        let mut rebuild = RebuildMetrics::default();
        let (selection, loaded) = match current_manifest {
            Some(current) => {
                let selection = select_manifest_history(&catalog.required, &current, config)?;
                let loaded = Self::load_manifest_runs(&runs_dir, &current, config, options);
                generation = current.generation;
                let loaded = match loaded {
                    Ok(loaded) => {
                        reconcile_segment_files(root, &catalog.entries, &selection)?;
                        clear_stale_temp_files(root)?;
                        clear_rebuild_staging(&runs_dir)?;
                        loaded
                    }
                    Err(error) if is_non_rebuildable_run_error(&error) => return Err(error),
                    Err(error) => {
                        // Indexes are derived, but only the manifest's exact
                        // visible prefix may be rebuilt without an external
                        // canonical marker. Raw frames beyond it stay orphaned.
                        let rebuild_generation = next_rebuilt_manifest_generation(
                            newest_manifest.as_ref().map(|(generation, _)| *generation),
                        )?;
                        let plan = Self::preflight_rebuild_runs_from_segments(
                            &selection.segments,
                            config,
                        )?;
                        reconcile_segment_files(root, &catalog.entries, &selection)?;
                        clear_stale_temp_files(root)?;
                        let rebuild_started = Instant::now();
                        let loaded = Self::rebuild_runs_from_segments(
                            &selection.segments,
                            &runs_dir,
                            options,
                            config,
                            &plan,
                        )
                        .with_context(|| format!("rebuild manifest index runs: {error:#}"))?;
                        rebuild = RebuildMetrics {
                            frames: selection.frame_count,
                            runs: u64::try_from(loaded.runs.len())
                                .context("rebuild run count does not fit u64")?,
                            index_entries: loaded.index_entries,
                            wall_ns: duration_ns(rebuild_started.elapsed()),
                        };
                        generation = publish_rebuilt_manifest(
                            root,
                            rebuild_generation,
                            &selection.extents,
                            &loaded,
                        )?;
                        loaded
                    }
                };
                (selection, loaded)
            }
            // Without a manifest or an explicit external horizon there is no
            // durable commit decision. Complete frames and runs are prepared
            // orphan data and must remain invisible.
            None => {
                let selection = select_empty_history(&catalog.required)?;
                reconcile_segment_files(root, &catalog.entries, &selection)?;
                clear_stale_temp_files(root)?;
                clear_rebuild_staging(&runs_dir)?;
                (selection, LoadedRuns::default())
            }
        };
        Self::finish_open(
            root,
            selection,
            generation,
            loaded,
            config,
            rebuild,
            writer_lease,
        )
    }

    /// Loads every run listed in one manifest generation and cross-checks
    /// record counts and records checksums against the manifest entries.
    fn load_manifest_runs(
        runs_dir: &Path,
        current: &Manifest,
        config: PackStoreConfig,
        options: PackStoreOptions,
    ) -> Result<LoadedRuns> {
        preflight_manifest_runs(current, config)?;
        let max_index_memory_bytes = config.max_index_memory_bytes();
        let mut loaded = LoadedRuns::default();
        for entry in &current.entries {
            let run = read_manifest_index_run(&runs_dir.join(&entry.file_name), entry, options)?;
            ensure!(
                run.format_version == entry.format_version
                    && run.epoch == entry.max_epoch
                    && run.record_count == entry.record_count
                    && run.records_offset == entry.records_offset
                    && run.file_bytes == entry.file_bytes
                    && run.records_sha256 == entry.records_sha256,
                "manifest entry does not match run {}",
                entry.file_name
            );
            ensure!(
                run.structure_sha256 == entry.structure_sha256,
                "manifest entry does not match run {}",
                entry.file_name
            );
            verify_run(&run)
                .with_context(|| format!("verify committed index run {}", entry.file_name))?;
            charge_run_memory(&mut loaded, &run, max_index_memory_bytes)?;
            loaded.runs.push(LiveRun {
                run: Arc::new(run),
                level: entry.level,
                min_epoch: entry.min_epoch,
                max_epoch: entry.max_epoch,
            });
        }
        Ok(loaded)
    }

    /// Proves that every selected frame is authenticated, structurally
    /// decodable, and rebuildable within the cumulative resident-index bound.
    /// This must complete before recovery truncates or replaces any artifact.
    fn preflight_rebuild_runs_from_segments(
        segments: &[SelectedSegment],
        config: PackStoreConfig,
    ) -> Result<RebuildPlan> {
        let segment_count =
            u64::try_from(segments.len()).context("recovery segment count does not fit u64")?;
        let maximum_segments = manifest::HARD_MAX_MANIFEST_EXTENTS as u64;
        if segment_count > maximum_segments {
            return Err(PackStoreError::LimitExceeded {
                limit: PackStoreLimit::Segments,
                actual: segment_count,
                maximum: maximum_segments,
            }
            .into());
        }
        let mut planner = RebuildPlanner::new(config);
        walk_selected_frames(
            segments,
            config,
            |file, segment_id, epoch, frame_start, frame_end, header| {
                let _ = read_frame_receipt_at(file, segment_id, epoch, frame_start, frame_end)
                    .with_context(|| {
                        format!("authenticate frame {epoch} before recovery mutation")
                    })?;
                let rows = usize::try_from(header.rows)
                    .context("preflight row count does not fit usize")?;
                let metadata_len = usize::try_from(header.metadata_bytes)
                    .context("preflight metadata length does not fit usize")?;
                let metadata_start = frame_start
                    .checked_add(FRAME_HEADER_LEN as u64)
                    .context("preflight metadata offset overflows")?;
                let mut metadata = Vec::new();
                metadata
                    .try_reserve_exact(metadata_len)
                    .context("reserve frame metadata for recovery preflight")?;
                metadata.resize(metadata_len, 0);
                file.read_exact_at(&mut metadata, metadata_start)
                    .context("read frame metadata for recovery preflight")?;
                ensure!(
                    frame_metadata_digest(&metadata) == header.metadata_sha256,
                    "frame metadata checksum mismatch during recovery preflight"
                );
                let distinct =
                    scan_frame_metadata_distinct_keys(&metadata, rows, header.value_bytes)?;
                planner.push_frame(epoch, header.metadata_bytes, header.rows, distinct)
            },
        )?;
        planner.finish()
    }

    /// Rebuilds a bounded binary-carry run set directly from committed frames
    /// in an isolated staging directory. Canonical run names are replaced only
    /// after every staged output is durable and verified.
    fn rebuild_runs_from_segments(
        segments: &[SelectedSegment],
        runs_dir: &Path,
        options: PackStoreOptions,
        config: PackStoreConfig,
        plan: &RebuildPlan,
    ) -> Result<LoadedRuns> {
        let max_index_memory_bytes = config.max_index_memory_bytes();
        let staging_dir = prepare_rebuild_staging(runs_dir)?;
        let mut loaded = LoadedRuns::default();
        walk_selected_frames(
            segments,
            config,
            |file, segment_id, epoch, frame_start, frame_end, header| {
                let _ = read_frame_receipt_at(file, segment_id, epoch, frame_start, frame_end)
                    .with_context(|| format!("authenticate frame {epoch} before index rebuild"))?;
                let rows =
                    usize::try_from(header.rows).context("frame row count does not fit usize")?;
                let metadata_len = usize::try_from(header.metadata_bytes)
                    .context("frame metadata length does not fit usize")?;
                let minimum_workspace = rebuild_decode_workspace_bytes(
                    loaded.decoded_index_bytes,
                    header.metadata_bytes,
                    header.rows,
                )?;
                ensure_rebuild_memory_bound(minimum_workspace, max_index_memory_bytes)?;

                let metadata_start = frame_start
                    .checked_add(FRAME_HEADER_LEN as u64)
                    .context("rebuilt metadata offset overflows")?;
                let mut metadata = Vec::new();
                metadata
                    .try_reserve_exact(metadata_len)
                    .context("reserve frame metadata for index rebuild")?;
                metadata.resize(metadata_len, 0);
                file.read_exact_at(&mut metadata, metadata_start)
                    .context("read frame metadata for index rebuild")?;
                ensure!(
                    frame_metadata_digest(&metadata) == header.metadata_sha256,
                    "frame metadata checksum mismatch during index rebuild"
                );
                let distinct =
                    scan_frame_metadata_distinct_keys(&metadata, rows, header.value_bytes)?;
                let estimated_peak = estimate_rebuild_peak_bytes(
                    loaded.decoded_index_bytes,
                    header.metadata_bytes,
                    header.rows,
                    distinct,
                )?;
                ensure_rebuild_memory_bound(estimated_peak, max_index_memory_bytes)?;
                let entries = frame_codec::decode_frame_metadata_in_segment(
                    segment_id,
                    frame_start,
                    &metadata,
                    header.value_bytes,
                )?;
                drop(metadata);
                let file_name = run_file_name(0, epoch, epoch);
                let run = publish_fresh_run(&entries, epoch, &staging_dir, &file_name, options)
                    .with_context(|| format!("rebuild index run for frame {epoch}"))?;
                verify_run(&run)
                    .with_context(|| format!("verify rebuilt index run for frame {epoch}"))?;
                charge_run_memory(&mut loaded, &run, max_index_memory_bytes)?;
                loaded.runs.push(LiveRun {
                    run: Arc::new(run),
                    level: 0,
                    min_epoch: epoch,
                    max_epoch: epoch,
                });
                compact_rebuild_carry(&mut loaded, &staging_dir, options, max_index_memory_bytes)?;
                Ok(())
            },
        )?;
        validate_rebuild_result(&loaded, plan)?;
        promote_rebuilt_runs(runs_dir, &staging_dir, &loaded)?;
        reload_promoted_runs(runs_dir, &loaded, options, max_index_memory_bytes)
    }

    /// Shared open tail: truncate everything past the committed frame prefix,
    /// map the pack, and fully verify every committed frame and index run.
    fn finish_open(
        root: &Path,
        selection: SegmentSelection,
        generation: u64,
        loaded: LoadedRuns,
        config: PackStoreConfig,
        rebuild: RebuildMetrics,
        writer_lease: File,
    ) -> Result<Self> {
        let options = config.read_options();
        let committed_frames = selection.extents.iter().try_fold(0u64, |total, extent| {
            total
                .checked_add(extent.frame_count)
                .context("committed frame count overflows")
        })?;
        ensure!(
            committed_frames == selection.frame_count,
            "selected segment history frame count differs from its extents"
        );
        let active_segment_id = selection.tip_id();
        let active_segment_end = selection.tip_end();
        let last_frame_receipt = selection.last_frame_receipt;
        let extents = selection.extents;
        drop(selection.segments);

        let (pack, pack_path) = open_segment_for_append(root, active_segment_id)?;
        ensure!(
            pack.metadata().context("stat active append segment")?.len() == active_segment_end,
            "active segment length changed after recovery authentication"
        );
        let segments = Arc::new(SegmentSet::open(
            root,
            &extents,
            options,
            config.max_segment_bytes(),
        )?);
        let ranges = loaded
            .runs
            .iter()
            .map(|live| RunRange {
                min_prefix: live.run.min_prefix,
                max_prefix: live.run.max_prefix,
            })
            .collect();
        let frames = committed_frames;
        let run_count = u64::try_from(loaded.runs.len()).context("run count does not fit u64")?;
        let stats = CompactionStats {
            peak_live_runs: run_count,
            ..CompactionStats::default()
        };
        let level_run_counts = count_run_levels(&loaded.runs);
        Ok(Self {
            root: root.to_path_buf(),
            runs_dir: root.join("runs"),
            pack,
            pack_path,
            active_segment_id,
            segments,
            runs: loaded.runs,
            level_run_counts,
            ranges,
            next_epoch: frames,
            generation,
            extents,
            decoded_index_bytes: loaded.decoded_index_bytes,
            config,
            stats,
            rebuild,
            stage_totals: PackStageTotals::default(),
            logical_payload_bytes: 0,
            read_counters: Arc::new(ReadCounters::default()),
            leases: Arc::new(Mutex::new(BTreeMap::new())),
            open_validation: OpenValidation {
                frames,
                runs: run_count,
                index_entries: loaded.index_entries,
            },
            last_frame_receipt,
            pending_append: None,
            instance_id: next_store_instance_id(),
            next_prepare_serial: 0,
            _writer_lease: writer_lease,
            inflight_compaction_outputs: Arc::new(Mutex::new(HashSet::new())),
        })
    }
}

pub(super) const REBUILD_STAGING_DIRECTORY: &str = ".rebuild-staging-v1";

/// Walks selected frame prefixes with one segment descriptor open at a time.
/// The frame codec continues scanning any suffix for unsupported versions,
/// while the callback sees only the authenticated canonical prefix.
fn walk_selected_frames(
    segments: &[SelectedSegment],
    config: PackStoreConfig,
    mut visit: impl FnMut(&File, PackSegmentId, u64, u64, u64, frame_codec::FrameHeader) -> Result<()>,
) -> Result<()> {
    for segment in segments {
        let file = File::open(&segment.path)
            .with_context(|| format!("open selected segment {}", segment.id))?;
        walk_frames_from_epoch(
            &file,
            segment.first_epoch,
            Some(FrameWalkSelection {
                frame_count: segment.frame_count,
                frame_end: segment.committed_end,
            }),
            |epoch, frame_start, frame_end, header| {
                validate_frame_resource_bounds(header, config)?;
                visit(&file, segment.id, epoch, frame_start, frame_end, header)
            },
        )
        .with_context(|| format!("walk selected segment {}", segment.id))?;
    }
    Ok(())
}

fn prepare_rebuild_staging(runs_dir: &Path) -> Result<PathBuf> {
    let staging = runs_dir.join(REBUILD_STAGING_DIRECTORY);
    clear_rebuild_staging(runs_dir)?;
    fs::create_dir(&staging)
        .with_context(|| format!("create recovery staging {}", staging.display()))?;
    sync_directory(runs_dir)?;
    Ok(staging)
}

fn clear_rebuild_staging(runs_dir: &Path) -> Result<()> {
    let staging = runs_dir.join(REBUILD_STAGING_DIRECTORY);
    match fs::symlink_metadata(&staging) {
        Ok(metadata) => {
            ensure!(
                metadata.file_type().is_dir() && !metadata.file_type().is_symlink(),
                "recovery staging path is not an owned directory: {}",
                staging.display()
            );
            fs::remove_dir_all(&staging).with_context(|| {
                format!("remove interrupted recovery staging {}", staging.display())
            })?;
            sync_directory(runs_dir)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect recovery staging {}", staging.display()));
        }
    }
    Ok(())
}

fn compact_rebuild_carry(
    loaded: &mut LoadedRuns,
    staging_dir: &Path,
    options: PackStoreOptions,
    max_index_memory_bytes: u64,
) -> Result<()> {
    while loaded.runs.len() >= 2 {
        let split = loaded.runs.len() - 2;
        let level = loaded.runs[split].level;
        if loaded.runs[split + 1].level != level {
            break;
        }
        let inputs = loaded.runs[split..].to_vec();
        let pending = build_compacted_run_from_inputs(
            level,
            &inputs,
            staging_dir,
            options.random_point_mmap,
            loaded.decoded_index_bytes,
            max_index_memory_bytes,
        )?;
        verify_run(&pending.run).context("verify recovery carry output")?;
        let decoded_index_bytes = loaded
            .decoded_index_bytes
            .checked_sub(pending.input_memory_bytes)
            .and_then(|bytes| bytes.checked_add(pending.run.memory_bytes))
            .context("recovery carry decoded index bytes overflow")?;
        ensure_rebuild_memory_bound(decoded_index_bytes, max_index_memory_bytes)?;
        let index_entries = loaded
            .index_entries
            .checked_sub(pending.input_records)
            .and_then(|entries| entries.checked_add(pending.output_records))
            .context("recovery carry index entry count overflow")?;
        let output = LiveRun {
            run: Arc::clone(&pending.run),
            level: pending.level,
            min_epoch: pending.min_epoch,
            max_epoch: pending.max_epoch,
        };

        for input in &inputs {
            let path =
                staging_dir.join(run_file_name(input.level, input.min_epoch, input.max_epoch));
            fs::remove_file(&path)
                .with_context(|| format!("remove recovery carry input {}", path.display()))?;
        }
        loaded.runs.truncate(split);
        loaded.runs.push(output);
        loaded.decoded_index_bytes = decoded_index_bytes;
        loaded.index_entries = index_entries;
    }
    Ok(())
}

fn validate_rebuild_result(loaded: &LoadedRuns, plan: &RebuildPlan) -> Result<()> {
    ensure!(
        loaded.runs.len() == plan.runs.len(),
        "recovery run count differs from its preflight plan"
    );
    ensure!(
        loaded.decoded_index_bytes <= plan.peak_index_bytes,
        "recovery resident index bytes exceed its preflight peak"
    );
    for (actual, planned) in loaded.runs.iter().zip(&plan.runs) {
        ensure!(
            actual.level == planned.level
                && actual.min_epoch == planned.min_epoch
                && actual.max_epoch == planned.max_epoch,
            "recovery run shape differs from its preflight plan"
        );
        ensure!(
            actual.run.record_count <= planned.record_count
                && actual.run.memory_bytes <= planned.memory_bytes,
            "recovery run size exceeds its preflight upper bound"
        );
    }
    Ok(())
}

fn promote_rebuilt_runs(runs_dir: &Path, staging_dir: &Path, loaded: &LoadedRuns) -> Result<()> {
    sync_directory(staging_dir)?;
    crate::engine::failpoint::crash("recovery.rebuild.after-staging-sync");
    for live in &loaded.runs {
        let file_name = run_file_name(live.level, live.min_epoch, live.max_epoch);
        let source = staging_dir.join(&file_name);
        let destination = runs_dir.join(&file_name);
        fs::rename(&source, &destination).with_context(|| {
            format!(
                "promote rebuilt run {} as {}",
                source.display(),
                destination.display()
            )
        })?;
        crate::engine::failpoint::crash("recovery.rebuild.after-run-promotion");
    }
    sync_directory(runs_dir)?;
    crate::engine::failpoint::crash("recovery.rebuild.after-run-directory-sync");
    fs::remove_dir(staging_dir).with_context(|| {
        format!(
            "remove empty recovery staging directory {}",
            staging_dir.display()
        )
    })?;
    sync_directory(runs_dir)?;
    Ok(())
}

fn reload_promoted_runs(
    runs_dir: &Path,
    staged: &LoadedRuns,
    options: PackStoreOptions,
    max_index_memory_bytes: u64,
) -> Result<LoadedRuns> {
    let mut loaded = LoadedRuns::default();
    for live in &staged.runs {
        let file_name = run_file_name(live.level, live.min_epoch, live.max_epoch);
        let run = read_index_run_with_options(&runs_dir.join(&file_name), options)
            .with_context(|| format!("reopen promoted recovery run {file_name}"))?;
        verify_run(&run).with_context(|| format!("verify promoted recovery run {file_name}"))?;
        charge_run_memory(&mut loaded, &run, max_index_memory_bytes)?;
        loaded.runs.push(LiveRun {
            run: Arc::new(run),
            level: live.level,
            min_epoch: live.min_epoch,
            max_epoch: live.max_epoch,
        });
    }
    ensure!(
        loaded.index_entries == staged.index_entries,
        "promoted recovery index entry count changed during canonical reopen"
    );
    Ok(loaded)
}

/// Rejects manifest-selected run metadata before any run file is opened or
/// mapped. The estimate exactly matches the decoded fences, fixed filter
/// descriptor, and per-run metadata charged after mapping.
fn preflight_manifest_runs(current: &Manifest, config: PackStoreConfig) -> Result<()> {
    let run_count =
        u64::try_from(current.entries.len()).context("manifest run count does not fit u64")?;
    let maximum_runs = u64::try_from(config.max_recent_runs())
        .context("configured recent-run limit does not fit u64")?;
    if run_count > maximum_runs {
        return Err(PackStoreError::LimitExceeded {
            limit: PackStoreLimit::RecentRuns,
            actual: run_count,
            maximum: maximum_runs,
        }
        .into());
    }

    let level_count = current.entries.iter().try_fold(0u64, |maximum, entry| {
        u64::from(entry.level)
            .checked_add(1)
            .map(|count| maximum.max(count))
            .context("manifest index level count overflows")
    })?;
    let maximum_levels = u64::from(config.max_index_levels());
    if level_count > maximum_levels {
        return Err(PackStoreError::LimitExceeded {
            limit: PackStoreLimit::IndexLevels,
            actual: level_count,
            maximum: maximum_levels,
        }
        .into());
    }

    let decoded_index_bytes = current.entries.iter().try_fold(0u64, |total, entry| {
        preflight_manifest_run_layout(entry)?;
        total
            .checked_add(manifest_run_memory_bytes(entry.record_count)?)
            .context("manifest decoded index bytes overflow")
    })?;
    ensure_rebuild_memory_bound(decoded_index_bytes, config.max_index_memory_bytes())
}

fn preflight_manifest_run_layout(entry: &ManifestEntry) -> Result<()> {
    let fence_count = entry.record_count.div_ceil(FENCE_INTERVAL as u64);
    u32::try_from(fence_count).context("manifest run fence count does not fit u32")?;
    usize::try_from(entry.record_count).context("manifest run record count does not fit usize")?;
    let fence_bytes = fence_count
        .checked_mul(FENCE_KEY_BYTES as u64)
        .context("manifest run fence bytes overflow")?;
    let filter_bytes = blocked_bloom_bytes(entry.record_count)?;
    let records_offset = (INDEX_HEADER_LEN as u64)
        .checked_add(fence_bytes)
        .and_then(|bytes| bytes.checked_add(filter_bytes))
        .context("manifest run records offset overflows")?;
    let file_bytes = records_offset
        .checked_add(
            entry
                .record_count
                .checked_mul(INDEX_RECORD_LEN as u64)
                .context("manifest run record bytes overflow")?,
        )
        .context("manifest run file bytes overflow")?;
    usize::try_from(records_offset).context("manifest run records offset does not fit usize")?;
    usize::try_from(file_bytes).context("manifest run file bytes do not fit usize")?;
    ensure!(
        entry.records_offset == records_offset && entry.file_bytes == file_bytes,
        "manifest run geometry does not match its record count"
    );
    Ok(())
}

fn manifest_run_memory_bytes(record_count: u64) -> Result<u64> {
    ensure!(record_count > 0, "manifest run has no records");
    record_count
        .div_ceil(FENCE_INTERVAL as u64)
        .checked_mul(FENCE_KEY_BYTES as u64)
        .and_then(|bytes| bytes.checked_add(32))
        .and_then(|bytes| bytes.checked_add(RUN_METADATA_BYTES))
        .context("manifest run metadata bytes overflow")
}

fn rebuild_decode_workspace_bytes(
    resident_bytes: u64,
    metadata_bytes: u64,
    rows: u64,
) -> Result<u64> {
    resident_bytes
        .checked_add(metadata_bytes)
        .and_then(|bytes| bytes.checked_add(rows))
        .context("recovery metadata workspace estimate overflows")
}

/// Conservative phase-aware heap peak for rebuilding one current derived run from
/// authenticated v2 frame metadata. This includes the already resident run
/// generation and every allocation family used by metadata decode, Bloom-filter
/// construction, run encoding, and validating readback.
pub(super) fn estimate_rebuild_peak_bytes(
    resident_bytes: u64,
    metadata_bytes: u64,
    rows: u64,
    distinct_keys: usize,
) -> Result<u64> {
    let rows_usize = usize::try_from(rows).context("recovery row count does not fit usize")?;
    ensure!(
        distinct_keys > 0 && distinct_keys <= rows_usize,
        "recovery distinct-key count is outside the frame row set"
    );
    let entries = allocation_bytes(rows_usize, std::mem::size_of::<IndexEntry>())?;
    let sequence_bitmap = allocation_bytes(rows_usize, std::mem::size_of::<bool>())?;
    let decode_peak = metadata_bytes
        .checked_add(entries)
        .and_then(|bytes| bytes.checked_add(sequence_bitmap))
        .context("recovery decode workspace estimate overflows")?;

    let fence_count = rows_usize.div_ceil(FENCE_INTERVAL);
    let fences = allocation_bytes(fence_count, FENCE_KEY_BYTES)?;
    let filter_bytes = blocked_bloom_bytes(rows)?;
    let filter_build_peak = checked_sum(&[entries, fences, filter_bytes])?;

    let record_bytes = allocation_bytes(rows_usize, INDEX_RECORD_LEN)?;
    let encoded_output =
        checked_sum(&[INDEX_HEADER_LEN as u64, fences, filter_bytes, record_bytes])?;
    let encode_peak = checked_sum(&[entries, fences, filter_bytes, encoded_output])?;

    let readback_structured = run_structured_bytes(rows_usize, distinct_keys)?;
    let readback_peak = checked_sum(&[
        entries,
        fences,
        filter_bytes,
        encoded_output,
        readback_structured,
    ])?;
    resident_bytes
        .checked_add(
            decode_peak
                .max(filter_build_peak)
                .max(encode_peak)
                .max(readback_peak),
        )
        .context("recovery peak memory estimate overflows")
}

fn allocation_bytes(count: usize, item_bytes: usize) -> Result<u64> {
    let bytes = count
        .checked_mul(item_bytes)
        .context("recovery allocation estimate overflows usize")?;
    u64::try_from(bytes).context("recovery allocation estimate does not fit u64")
}

fn checked_sum(parts: &[u64]) -> Result<u64> {
    parts.iter().try_fold(0u64, |total, bytes| {
        total
            .checked_add(*bytes)
            .context("recovery workspace estimate overflows")
    })
}

fn ensure_rebuild_memory_bound(estimated: u64, maximum: u64) -> Result<()> {
    if estimated > maximum {
        return Err(PackStoreError::LimitExceeded {
            limit: PackStoreLimit::IndexMemoryBytes,
            actual: estimated,
            maximum,
        }
        .into());
    }
    Ok(())
}

fn is_unsupported_artifact_version(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<PackStoreError>()
        .is_some_and(|error| matches!(error, PackStoreError::UnsupportedVersion { .. }))
}

fn is_non_rebuildable_run_error(error: &anyhow::Error) -> bool {
    error.downcast_ref::<PackStoreError>().is_some_and(|error| {
        matches!(
            error,
            PackStoreError::UnsupportedVersion { .. } | PackStoreError::LimitExceeded { .. }
        )
    })
}

/// Authenticates the required segment prefix and classifies later orphan
/// segments without changing a directory entry or retaining orphan resources.
fn scan_segment_catalog(
    root: &Path,
    required_tip: PackSegmentId,
    config: PackStoreConfig,
) -> Result<ScannedCatalog> {
    let entries = discover_segment_catalog(root)?;
    ensure!(!entries.is_empty(), "required pack segment 0 is missing");
    let required_count = required_segment_prefix(&entries, required_tip)?.len();
    let mut first_epoch = 0u64;
    let mut required = Vec::with_capacity(required_count);
    for entry in &entries[..required_count] {
        let segment_first_epoch = first_epoch;
        let (file, path, file_bytes) = open_bounded_segment(root, entry, config)?;
        let scan = walk_frames_from_epoch(&file, first_epoch, None, |_, _, _, _| Ok(()))
            .with_context(|| format!("scan pack segment {}", entry.id))?;
        first_epoch = first_epoch
            .checked_add(scan.complete_count)
            .context("segment frame epoch overflows")?;
        required.push(ScannedSegment {
            id: entry.id,
            path,
            first_epoch: segment_first_epoch,
            complete_frames: scan.complete_count,
            complete_end: scan.last_end,
            file_bytes,
        });
    }

    let mut orphan_complete_frames = 0u64;
    for entry in &entries[required_count..] {
        let (file, _, _) = open_bounded_segment(root, entry, config)?;
        let scan = walk_frames_from_epoch(&file, first_epoch, None, |_, _, _, _| Ok(()))
            .with_context(|| format!("classify orphan pack segment {}", entry.id))?;
        first_epoch = first_epoch
            .checked_add(scan.complete_count)
            .context("orphan segment frame epoch overflows")?;
        orphan_complete_frames = orphan_complete_frames
            .checked_add(scan.complete_count)
            .context("orphan segment frame count overflows")?;
    }
    Ok(ScannedCatalog {
        entries,
        required,
        orphan_complete_frames,
    })
}

fn open_bounded_segment(
    root: &Path,
    entry: &SegmentCatalogEntry,
    config: PackStoreConfig,
) -> Result<(File, PathBuf, u64)> {
    let (file, path) = open_segment_read_only(root, entry.id)?;
    ensure!(
        path == entry.path,
        "segment discovery path changed during open"
    );
    let file_bytes = file
        .metadata()
        .with_context(|| format!("stat pack segment {}", path.display()))?
        .len();
    if file_bytes > config.max_segment_bytes() {
        return Err(PackStoreError::LimitExceeded {
            limit: PackStoreLimit::SegmentBytes,
            actual: file_bytes,
            maximum: config.max_segment_bytes(),
        }
        .into());
    }
    Ok((file, path, file_bytes))
}

fn select_empty_history(catalog: &[ScannedSegment]) -> Result<SegmentSelection> {
    let _ = required_scanned_prefix(catalog, PackSegmentId::INITIAL)?;
    let initial = &catalog[0];
    Ok(SegmentSelection {
        segments: vec![SelectedSegment {
            id: initial.id,
            path: initial.path.clone(),
            first_epoch: 0,
            frame_count: 0,
            committed_end: SEGMENT_HEADER_LEN as u64,
        }],
        extents: Vec::new(),
        frame_count: 0,
        last_frame_receipt: None,
    })
}

fn select_manifest_history(
    catalog: &[ScannedSegment],
    current: &Manifest,
    config: PackStoreConfig,
) -> Result<SegmentSelection> {
    let tip = current
        .extents
        .last()
        .context("manifest has no segment extents")?
        .segment_id;
    let prefix = required_scanned_prefix(catalog, tip)?;
    ensure!(
        prefix.len() == current.extents.len(),
        "manifest segment extent count differs from its required segment prefix"
    );
    let mut selected = Vec::with_capacity(prefix.len());
    for (index, (segment, extent)) in prefix.iter().zip(&current.extents).enumerate() {
        ensure!(
            segment.id == extent.segment_id && segment.first_epoch == extent.first_epoch,
            "manifest generation {} does not bind the selected frame history",
            current.generation
        );
        ensure!(
            extent.frame_count <= segment.complete_frames,
            "manifest generation {} commits {} frames in segment {} but only {} are complete",
            current.generation,
            extent.frame_count,
            segment.id,
            segment.complete_frames
        );
        if index + 1 < prefix.len() {
            ensure!(
                segment.file_bytes == extent.frame_end
                    && segment.complete_frames == extent.frame_count
                    && segment.complete_end == extent.frame_end,
                "committed segment {} has an incomplete or orphan suffix before the manifest tip",
                segment.id
            );
        }
        selected.push(SelectedSegment {
            id: segment.id,
            path: segment.path.clone(),
            first_epoch: segment.first_epoch,
            frame_count: extent.frame_count,
            committed_end: extent.frame_end,
        });
    }
    let selection = authenticate_segment_selection(selected, config)?;
    ensure!(
        selection.extents == current.extents,
        "manifest generation {} does not bind the selected frame history",
        current.generation
    );
    Ok(selection)
}

fn select_commit_horizon(
    catalog: &[ScannedSegment],
    horizon: PackCommitHorizon,
    config: PackStoreConfig,
) -> Result<SegmentSelection> {
    let prefix = required_scanned_prefix(catalog, horizon.segment_id)?;
    let mut selected = Vec::with_capacity(prefix.len());
    for (index, segment) in prefix.iter().enumerate() {
        let is_tip = index + 1 == prefix.len();
        let selected_count = if is_tip {
            let local_epoch = horizon
                .epoch
                .checked_sub(segment.first_epoch)
                .with_context(|| {
                    format!(
                        "pack commit marker epoch {} precedes segment {}",
                        horizon.epoch, segment.id
                    )
                })?;
            local_epoch
                .checked_add(1)
                .context("marker segment frame count overflows")?
        } else {
            segment.complete_frames
        };
        ensure!(
            selected_count <= segment.complete_frames,
            "pack commit marker requires frame {} in segment {} but only {} complete frames exist",
            horizon.epoch,
            segment.id,
            segment.complete_frames
        );
        let committed_end = if is_tip {
            horizon.frame_end
        } else {
            segment.complete_end
        };
        if !is_tip {
            ensure!(
                selected_count > 0 && segment.file_bytes == committed_end,
                "committed segment {} is incomplete before marker segment {}",
                segment.id,
                horizon.segment_id
            );
        }
        selected.push(SelectedSegment {
            id: segment.id,
            path: segment.path.clone(),
            first_epoch: segment.first_epoch,
            frame_count: selected_count,
            committed_end,
        });
    }
    let selection = authenticate_segment_selection(selected, config)?;
    let receipt = selection
        .last_frame_receipt
        .context("pack commit marker frame is absent")?;
    ensure!(
        receipt.segment_id == horizon.segment_id,
        "pack commit marker segment {} does not match frame {} segment {}",
        horizon.segment_id,
        horizon.epoch,
        receipt.segment_id
    );
    ensure!(
        receipt.frame_end == horizon.frame_end,
        "pack commit marker end {} does not match frame {} end {}",
        horizon.frame_end,
        horizon.epoch,
        receipt.frame_end
    );
    ensure!(
        receipt.context == horizon.context,
        "pack commit marker context does not match frame {}",
        horizon.epoch
    );
    ensure!(
        receipt.frame_sha256 == horizon.frame_sha256,
        "pack commit marker frame digest does not match frame {}",
        horizon.epoch
    );
    Ok(selection)
}

fn required_scanned_prefix(
    catalog: &[ScannedSegment],
    tip: PackSegmentId,
) -> Result<&[ScannedSegment]> {
    let index = catalog
        .iter()
        .position(|segment| segment.id == tip)
        .with_context(|| format!("required pack segment {tip} is missing"))?;
    Ok(&catalog[..=index])
}

fn authenticate_segment_selection(
    segments: Vec<SelectedSegment>,
    config: PackStoreConfig,
) -> Result<SegmentSelection> {
    ensure!(!segments.is_empty(), "selected segment history is empty");
    let mut extents = Vec::new();
    let mut frame_count = 0u64;
    let mut last_frame_receipt = None;
    for segment in &segments {
        let committed_end = segment.committed_end();
        let file = File::open(&segment.path)
            .with_context(|| format!("open selected segment {}", segment.id))?;
        ensure!(
            file.metadata()
                .with_context(|| format!("stat selected segment {}", segment.id))?
                .len()
                >= committed_end,
            "selected segment {} length does not match frame prefix end {committed_end}",
            segment.id,
        );
        let map = Mmap::map_sequential(&file, committed_end, &segment.path)?;
        walk_frames_from_epoch(
            &file,
            segment.first_epoch,
            Some(FrameWalkSelection {
                frame_count: segment.frame_count,
                frame_end: committed_end,
            }),
            |epoch, frame_start, frame_end, header| {
                validate_frame_resource_bounds(header, config)?;
                let (receipt, _) = verify_frame(&map, segment.id, frame_start, frame_end, epoch)
                    .with_context(|| format!("authenticate committed frame {epoch}"))?;
                manifest::append_frame_extent(&mut extents, receipt)?;
                last_frame_receipt = Some(receipt);
                frame_count = frame_count
                    .checked_add(1)
                    .context("authenticated frame count overflows")?;
                Ok(())
            },
        )
        .with_context(|| format!("walk selected segment {}", segment.id))?;
    }
    Ok(SegmentSelection {
        segments,
        extents,
        frame_count,
        last_frame_receipt,
    })
}

fn validate_frame_resource_bounds(
    header: frame_codec::FrameHeader,
    config: PackStoreConfig,
) -> Result<()> {
    if header.rows > config.max_frame_rows() {
        return Err(PackStoreError::LimitExceeded {
            limit: PackStoreLimit::FrameRows,
            actual: header.rows,
            maximum: config.max_frame_rows(),
        }
        .into());
    }
    let payload_bytes = header
        .metadata_bytes
        .checked_add(header.value_bytes)
        .context("frame payload length overflows")?;
    if payload_bytes > config.max_frame_payload_bytes() {
        return Err(PackStoreError::LimitExceeded {
            limit: PackStoreLimit::FramePayloadBytes,
            actual: payload_bytes,
            maximum: config.max_frame_payload_bytes(),
        }
        .into());
    }
    if header.frame_bytes > config.max_pending_bytes() {
        return Err(PackStoreError::LimitExceeded {
            limit: PackStoreLimit::PendingBytes,
            actual: header.frame_bytes,
            maximum: config.max_pending_bytes(),
        }
        .into());
    }
    let isolated_segment_bytes = PACK_SEGMENT_HEADER_LEN
        .checked_add(header.frame_bytes)
        .context("isolated frame segment length overflows")?;
    if isolated_segment_bytes > config.max_segment_bytes() {
        return Err(PackStoreError::LimitExceeded {
            limit: PackStoreLimit::SegmentBytes,
            actual: isolated_segment_bytes,
            maximum: config.max_segment_bytes(),
        }
        .into());
    }
    Ok(())
}

fn reconcile_segment_files(
    root: &Path,
    catalog: &[SegmentCatalogEntry],
    selection: &SegmentSelection,
) -> Result<()> {
    let tip = selection.tip_id();
    let tip_end = selection.tip_end();
    let tip_segment = catalog
        .iter()
        .find(|segment| segment.id == tip)
        .context("selected tip segment disappeared from the recovery catalog")?;
    let tip_bytes = fs::metadata(&tip_segment.path)
        .with_context(|| format!("stat selected tip segment {tip}"))?
        .len();
    ensure!(
        tip_bytes >= tip_end,
        "selected tip segment {tip} became shorter after authentication"
    );
    if tip_bytes != tip_end {
        let writable = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&tip_segment.path)
            .with_context(|| format!("open segment {} for recovery truncation", tip))?;
        writable
            .set_len(tip_end)
            .with_context(|| format!("truncate segment {tip} to committed prefix"))?;
        writable
            .sync_data()
            .with_context(|| format!("sync truncated segment {tip}"))?;
    }
    for segment in catalog.iter().filter(|segment| segment.id > tip) {
        fs::remove_file(&segment.path)
            .with_context(|| format!("remove orphan segment {}", segment.id))?;
        sync_directory(root)?;
    }
    Ok(())
}

/// Removes only derived visibility artifacts after payload authentication and
/// rebuild preflight have succeeded. Segment reconciliation is deliberately a
/// separate operation so payload authority cannot be hidden in index cleanup.
fn clear_derived_visibility(root: &Path) -> Result<()> {
    let runs_dir = root.join("runs");
    for entry in fs::read_dir(&runs_dir)
        .with_context(|| format!("read index-run directory {}", runs_dir.display()))?
    {
        let entry = entry.context("read index-run recovery entry")?;
        let path = entry.path();
        let remove = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                manifest::is_run_file_name(name) || manifest::is_run_temp_file_name(name)
            });
        if remove {
            fs::remove_file(&path)
                .with_context(|| format!("remove derived index run {}", path.display()))?;
        }
    }
    clear_rebuild_staging(&runs_dir)?;
    for entry in fs::read_dir(root)
        .with_context(|| format!("read pack root {} for recovery", root.display()))?
    {
        let entry = entry.context("read pack recovery entry")?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if manifest::parse_manifest_file_name(name).is_some()
            || manifest::is_manifest_temp_file_name(name)
        {
            fs::remove_file(&path)
                .with_context(|| format!("remove derived manifest {}", path.display()))?;
        }
    }
    sync_directory(&runs_dir)?;
    sync_directory(root)?;
    Ok(())
}
