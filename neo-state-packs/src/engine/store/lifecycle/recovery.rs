use super::segment::{SEGMENT_HEADER_LEN, create_initial_segment, open_initial_segment};
use super::*;

impl PackStore {
    /// Creates an empty pack store in `root` using one validated resource
    /// contract. The directory must be missing or empty.
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
            ensure!(
                fs::read_dir(root)
                    .with_context(|| format!("read pack store directory {}", root.display()))?
                    .next()
                    .is_none(),
                "pack store directory must be empty: {}",
                root.display()
            );
        } else {
            fs::create_dir_all(root)
                .with_context(|| format!("create pack store directory {}", root.display()))?;
            sync_parent_directory(root)?;
        }
        let writer_lease = acquire_writer_lease(root)?;
        let runs_dir = root.join("runs");
        fs::create_dir(&runs_dir)
            .with_context(|| format!("create index-run directory {}", runs_dir.display()))?;
        let (pack, pack_path) = create_initial_segment(root)?;
        let initial_len = SEGMENT_HEADER_LEN as u64;
        let pack_map = Mmap::map(&pack, initial_len, &pack_path)?;
        let lookup_pack_map = map_random_if_enabled(&pack, initial_len, &pack_path, options)?;
        Ok(Self {
            root: root.to_path_buf(),
            runs_dir,
            pack,
            pack_path,
            pack_map: Arc::new(pack_map),
            lookup_pack_map: lookup_pack_map.map(Arc::new),
            runs: Vec::new(),
            level_run_counts: BTreeMap::new(),
            ranges: Vec::new(),
            next_epoch: 0,
            generation: 0,
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
        let max_index_memory_bytes = config.max_index_memory_bytes();
        read_view::preflight_pack_value_pool(options.batch_value_workers)?;
        let writer_lease = acquire_writer_lease(root)?;
        let (pack, pack_path) = open_initial_segment(root)?;
        let scan = scan_frames(&pack)?;
        let expected_frames = match horizon {
            Some(horizon) => horizon
                .epoch
                .checked_add(1)
                .context("committed pack epoch overflows")?,
            None => 0,
        };
        ensure!(
            expected_frames <= scan.frame_ends.len() as u64,
            "pack commit marker requires {expected_frames} frames but only {} complete frames exist",
            scan.frame_ends.len()
        );
        if let Some(horizon) = horizon {
            ensure!(
                horizon.segment_id == PackSegmentId::INITIAL,
                "pack commit marker names unavailable segment {}",
                horizon.segment_id
            );
            let receipt = read_frame_receipt(&pack, &scan, horizon.epoch)?;
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
        }
        authenticate_committed_frame_prefix(&pack, &pack_path, &scan, expected_frames)?;
        let manifests = manifest::list_manifest_files(root)?;
        let current_manifest = manifests
            .first()
            .and_then(|(_, path)| manifest::read_manifest(path).ok());
        let manifest_frames = current_manifest
            .as_ref()
            .and_then(|manifest| manifest.max_epoch().checked_add(1));
        let manifest_runs_valid = current_manifest.as_ref().is_some_and(|manifest| {
            Self::load_manifest_runs(
                &root.join("runs"),
                manifest,
                max_index_memory_bytes,
                options,
            )
            .is_ok()
        });
        let fast_open = if expected_frames == 0 {
            scan.frame_ends.is_empty()
                && manifests.is_empty()
                && fs::read_dir(root.join("runs"))
                    .context("read index-run directory for empty horizon")?
                    .next()
                    .is_none()
        } else {
            manifest_frames == Some(expected_frames) && manifest_runs_valid
        };
        drop(pack);

        if !fast_open {
            reset_derived_state_to_frame_prefix(root, &scan, expected_frames)?;
            if expected_frames > 0 {
                let (recovered_pack, recovered_path) = open_initial_segment(root)?;
                ensure!(
                    recovered_path == pack_path,
                    "marker rebuild opened a different pack segment"
                );
                let recovered_scan = scan_frames(&recovered_pack)?;
                ensure!(
                    recovered_scan.frame_ends.len() as u64 == expected_frames,
                    "marker recovery retained {} frames, expected {expected_frames}",
                    recovered_scan.frame_ends.len()
                );
                let loaded = Self::rebuild_runs_from_frames(
                    &recovered_pack,
                    &recovered_scan.frame_ends,
                    &root.join("runs"),
                    max_index_memory_bytes,
                    options,
                )?;
                let manifests = manifest::list_manifest_files(root)?;
                ensure!(
                    manifests.is_empty(),
                    "marker recovery did not remove old manifests"
                );
                publish_rebuilt_manifest(root, &manifests, &loaded)?;
            }
        }

        let store = Self::open_with_lease(root, config, writer_lease)?;
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
        Ok(store)
    }

    fn open_inner(root: &Path, config: PackStoreConfig) -> Result<Self> {
        config.validate()?;
        let options = config.read_options().normalized_for_host();
        let config = config.with_read_options(options)?;
        read_view::preflight_pack_value_pool(options.batch_value_workers)?;
        let writer_lease = acquire_writer_lease(root)?;
        Self::open_with_lease(root, config, writer_lease)
    }

    fn open_with_lease(root: &Path, config: PackStoreConfig, writer_lease: File) -> Result<Self> {
        config.validate()?;
        let max_index_memory_bytes = config.max_index_memory_bytes();
        let options = config.read_options();
        let runs_dir = root.join("runs");
        let (pack, pack_path) = open_initial_segment(root)?;
        let scan = scan_frames(&pack)?;
        let frame_count =
            u64::try_from(scan.frame_ends.len()).context("frame count does not fit u64")?;
        let manifests = manifest::list_manifest_files(root)?;
        let mut generation = 0u64;
        let mut rebuild = RebuildMetrics::default();
        let loaded = match manifests.first() {
            Some((_, path)) => {
                let current = manifest::read_manifest(path).with_context(|| {
                    format!(
                        "read newest manifest {}; visibility authority is unavailable",
                        path.display()
                    )
                })?;
                ensure!(
                    current.max_epoch() < frame_count,
                    "manifest generation {} commits {} frames but only {} validated in the pack",
                    current.generation,
                    current.max_epoch() + 1,
                    frame_count
                );
                authenticate_committed_frame_prefix(
                    &pack,
                    &pack_path,
                    &scan,
                    current
                        .max_epoch()
                        .checked_add(1)
                        .context("manifest committed-frame count overflows")?,
                )?;
                let loaded =
                    Self::load_manifest_runs(&runs_dir, &current, max_index_memory_bytes, options);
                clear_stale_temp_files(root)?;
                generation = current.generation;
                match loaded {
                    Ok(loaded) => loaded,
                    Err(error) => {
                        // Indexes are derived, but only the manifest's exact
                        // visible prefix may be rebuilt without an external
                        // canonical marker. Raw frames beyond it stay orphaned.
                        let prefix = &scan.frame_ends[..=usize::try_from(current.max_epoch())
                            .context("manifest epoch does not fit usize")?];
                        let rebuild_started = Instant::now();
                        let loaded = Self::rebuild_runs_from_frames(
                            &pack,
                            prefix,
                            &runs_dir,
                            max_index_memory_bytes,
                            options,
                        )
                        .with_context(|| format!("rebuild manifest index runs: {error:#}"))?;
                        rebuild = RebuildMetrics {
                            frames: u64::try_from(prefix.len())
                                .context("rebuild frame count does not fit u64")?,
                            runs: u64::try_from(loaded.runs.len())
                                .context("rebuild run count does not fit u64")?,
                            index_entries: loaded.index_entries,
                            wall_ns: duration_ns(rebuild_started.elapsed()),
                        };
                        generation = publish_rebuilt_manifest(root, &manifests, &loaded)?;
                        loaded
                    }
                }
            }
            // Without a manifest or an explicit external horizon there is no
            // durable commit decision. Complete frames and runs are prepared
            // orphan data and must remain invisible.
            None => {
                clear_stale_temp_files(root)?;
                LoadedRuns::default()
            }
        };
        Self::finish_open(
            root,
            pack,
            pack_path,
            scan,
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
        max_index_memory_bytes: u64,
        options: PackStoreOptions,
    ) -> Result<LoadedRuns> {
        let mut loaded = LoadedRuns::default();
        for entry in &current.entries {
            let run = read_index_run_with_options(&runs_dir.join(&entry.file_name), options)?;
            ensure!(
                run.epoch == entry.max_epoch
                    && run.record_count == entry.record_count
                    && run.records_sha256 == entry.records_sha256,
                "manifest entry does not match run {}",
                entry.file_name
            );
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

    /// Rebuilds one level-0 run per committed frame directly from the pack.
    /// Every frame payload is re-hashed and decoded; this is the slow
    /// recovery path, never the steady-state open.
    fn rebuild_runs_from_frames(
        pack: &File,
        frame_ends: &[u64],
        runs_dir: &Path,
        max_index_memory_bytes: u64,
        options: PackStoreOptions,
    ) -> Result<LoadedRuns> {
        let mut loaded = LoadedRuns::default();
        let mut frame_start = SEGMENT_HEADER_LEN as u64;
        for (epoch, frame_end) in frame_ends.iter().enumerate() {
            let epoch = u64::try_from(epoch).context("rebuilt epoch does not fit u64")?;
            let _ = read_frame_receipt_at(pack, epoch, frame_start, *frame_end)
                .with_context(|| format!("authenticate frame {epoch} before index rebuild"))?;
            let mut header = [0u8; FRAME_HEADER_LEN];
            pack.read_exact_at(&mut header, frame_start)
                .context("re-read frame header for index rebuild")?;
            let header = validate_frame_header(&header, epoch)?;
            let rebuilt_frame_end = frame_start
                .checked_add(header.frame_bytes)
                .context("rebuilt frame end overflows")?;
            ensure!(
                rebuilt_frame_end == *frame_end,
                "rebuilt frame length mismatch at epoch {epoch}"
            );
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
            pack.read_exact_at(&mut metadata, metadata_start)
                .context("read frame metadata for index rebuild")?;
            ensure!(
                frame_metadata_digest(&metadata) == header.metadata_sha256,
                "frame metadata checksum mismatch during index rebuild"
            );
            let distinct = scan_frame_metadata_distinct_keys(&metadata, rows, header.value_bytes)?;
            let estimated_peak = estimate_rebuild_peak_bytes(
                loaded.decoded_index_bytes,
                header.metadata_bytes,
                header.rows,
                distinct,
            )?;
            ensure_rebuild_memory_bound(estimated_peak, max_index_memory_bytes)?;
            let entries = decode_frame_metadata(frame_start, &metadata, header.value_bytes)?;
            drop(metadata);
            let file_name = run_file_name(0, epoch, epoch);
            let run = publish_fresh_run(&entries, epoch, runs_dir, &file_name, options)
                .with_context(|| format!("rebuild index run for frame {epoch}"))?;
            charge_run_memory(&mut loaded, &run, max_index_memory_bytes)?;
            loaded.runs.push(LiveRun {
                run: Arc::new(run),
                level: 0,
                min_epoch: epoch,
                max_epoch: epoch,
            });
            frame_start = *frame_end;
        }
        Ok(loaded)
    }

    /// Shared open tail: truncate everything past the committed frame prefix,
    /// map the pack, and fully verify every committed frame and index run.
    fn finish_open(
        root: &Path,
        pack: File,
        pack_path: PathBuf,
        scan: FrameScan,
        generation: u64,
        loaded: LoadedRuns,
        config: PackStoreConfig,
        rebuild: RebuildMetrics,
        writer_lease: File,
    ) -> Result<Self> {
        let options = config.read_options();
        let last_frame_receipt = loaded
            .runs
            .last()
            .map(|tail| read_frame_receipt(&pack, &scan, tail.max_epoch))
            .transpose()?;
        let committed_end = match loaded.runs.last() {
            Some(tail) => {
                scan.frame_ends[usize::try_from(tail.max_epoch)
                    .context("committed epoch does not fit usize")?]
            }
            None => SEGMENT_HEADER_LEN as u64,
        };
        // A frame becomes visible only with its published manifest. Truncate
        // torn tail bytes and any frames whose publication was interrupted.
        if pack.metadata().context("stat append pack")?.len() != committed_end {
            pack.set_len(committed_end)
                .context("truncate append pack to committed frames")?;
            pack.sync_data().context("sync truncated append pack")?;
        }
        let pack_map = Mmap::map(&pack, committed_end, &pack_path)?;
        let lookup_pack_map = map_random_if_enabled(&pack, committed_end, &pack_path, options)?;
        let committed_frames = loaded.runs.last().map_or(0, |tail| tail.max_epoch + 1);
        for epoch in 0..committed_frames {
            let index = usize::try_from(epoch).context("frame epoch does not fit usize")?;
            let frame_end = *scan
                .frame_ends
                .get(index)
                .with_context(|| format!("missing committed frame {epoch}"))?;
            let frame_start = if index == 0 {
                SEGMENT_HEADER_LEN as u64
            } else {
                scan.frame_ends[index - 1]
            };
            verify_frame(&pack_map, frame_start, frame_end, epoch)
                .with_context(|| format!("verify committed frame {epoch}"))?;
        }
        for live in &loaded.runs {
            verify_run(&live.run).with_context(|| {
                format!(
                    "verify committed index run through epoch {}",
                    live.max_epoch
                )
            })?;
        }
        let ranges = loaded
            .runs
            .iter()
            .map(|live| RunRange {
                min_prefix: live.run.min_prefix,
                max_prefix: live.run.max_prefix,
            })
            .collect();
        let frames = loaded.runs.last().map_or(0, |tail| tail.max_epoch + 1);
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
            pack_map: Arc::new(pack_map),
            lookup_pack_map: lookup_pack_map.map(Arc::new),
            runs: loaded.runs,
            level_run_counts,
            ranges,
            next_epoch: frames,
            generation,
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
/// generation and every allocation family used by metadata decode, xor-filter
/// construction, duplicate run encoding, and validating readback.
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

    let keys = allocation_bytes(distinct_keys, PACK_KEY_BYTES)?;
    let fence_count = rows_usize.div_ceil(FENCE_INTERVAL);
    let fences = allocation_bytes(fence_count, FENCE_KEY_BYTES)?;
    let filter_slots = filter_capacity(distinct_keys);
    let key_hashes = allocation_bytes(distinct_keys, std::mem::size_of::<u64>())?;
    let xor_counts = allocation_bytes(filter_slots, std::mem::size_of::<u8>())?;
    let xor_values = allocation_bytes(filter_slots, std::mem::size_of::<u64>())?;
    let xor_queue = allocation_bytes(filter_slots, std::mem::size_of::<usize>())?;
    let xor_peeled = allocation_bytes(distinct_keys, std::mem::size_of::<(usize, u64)>())?;
    let filter_bytes = allocation_bytes(filter_slots, std::mem::size_of::<u16>())?;
    let filter_build_peak = checked_sum(&[
        entries,
        keys,
        fences,
        key_hashes,
        xor_counts,
        xor_values,
        xor_queue,
        xor_peeled,
        filter_bytes,
    ])?;

    let record_bytes = allocation_bytes(rows_usize, INDEX_RECORD_LEN)?;
    let encoded_output =
        checked_sum(&[INDEX_HEADER_LEN as u64, fences, filter_bytes, record_bytes])?;
    let encode_peak = checked_sum(&[
        entries,
        keys,
        fences,
        filter_bytes,
        record_bytes,
        filter_bytes,
        encoded_output,
    ])?;

    let readback_structured = run_structured_bytes(rows_usize, distinct_keys)?;
    let readback_peak = checked_sum(&[
        entries,
        keys,
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

/// Authenticates the complete marker- or manifest-selected frame prefix before
/// recovery mutates any pack-store artifact. Validation hashes both variable
/// sections, verifies the footer/full-frame digest, and parses every canonical
/// metadata row through a bounded sequential mapping.
fn authenticate_committed_frame_prefix(
    pack: &File,
    pack_path: &Path,
    scan: &FrameScan,
    committed_frames: u64,
) -> Result<()> {
    ensure!(
        committed_frames <= scan.frame_ends.len() as u64,
        "committed frame prefix requires {committed_frames} frames but only {} complete frames exist",
        scan.frame_ends.len()
    );
    if committed_frames == 0 {
        return Ok(());
    }
    let last_index = usize::try_from(committed_frames - 1)
        .context("committed frame count does not fit usize")?;
    let committed_end = *scan
        .frame_ends
        .get(last_index)
        .context("committed frame prefix is incomplete")?;
    let map = Mmap::map_sequential(pack, committed_end, pack_path)?;
    let mut frame_start = SEGMENT_HEADER_LEN as u64;
    for epoch in 0..committed_frames {
        let index = usize::try_from(epoch).context("committed frame epoch does not fit usize")?;
        let frame_end = *scan
            .frame_ends
            .get(index)
            .with_context(|| format!("committed frame {epoch} is absent"))?;
        verify_frame(&map, frame_start, frame_end, epoch)
            .with_context(|| format!("authenticate committed frame {epoch}"))?;
        frame_start = frame_end;
    }
    Ok(())
}
