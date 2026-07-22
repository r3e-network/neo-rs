use super::segment::SEGMENT_HEADER_LEN;
use super::*;

impl PackStore {
    /// Creates a bounded store-owned builder when aggregate value bytes are
    /// not known in advance.
    pub fn frame_builder(
        &self,
        context: PackFrameContext,
        expected_rows: usize,
    ) -> Result<PackFrameBuilder> {
        self.new_frame_builder(context, expected_rows, None)
    }

    /// Creates a bounded store-owned builder with an exact aggregate value
    /// byte count, allowing its final value buffer to be reserved once.
    pub fn frame_builder_with_value_bytes(
        &self,
        context: PackFrameContext,
        expected_rows: usize,
        value_bytes: u64,
    ) -> Result<PackFrameBuilder> {
        self.new_frame_builder(context, expected_rows, Some(value_bytes))
    }

    fn new_frame_builder(
        &self,
        context: PackFrameContext,
        expected_rows: usize,
        value_bytes: Option<u64>,
    ) -> Result<PackFrameBuilder> {
        validate_frame_context(context)?;
        let rows = u64::try_from(expected_rows).context("frame row count does not fit u64")?;
        let metadata_bytes = rows
            .checked_mul(FRAME_ROW_METADATA_LEN as u64)
            .context("frame metadata length overflows")?;
        self.ensure_frame_limits(rows, metadata_bytes, value_bytes.unwrap_or(0))?;
        // The builder reserves one pending row per operation. Prove the
        // worst-case all-distinct derived-index footprint before that reserve.
        self.ensure_index_memory(expected_rows, expected_rows)?;
        PackFrameBuilder::for_store(
            self.instance_id,
            context,
            expected_rows,
            value_bytes,
            self.config.max_frame_payload_bytes(),
            self.config.max_pending_bytes(),
        )
    }

    /// Durably prepares one contextual frame and immediately activates it
    /// through its matching commit horizon.
    ///
    /// If post-activation index maintenance fails, the returned error is
    /// [`PackStoreError::CommittedMaintenance`]. The frame is already visible
    /// and callers must not retry the same logical append through this handle.
    pub fn append_frame(
        &mut self,
        context: PackFrameContext,
        operations: &[PackOperation],
    ) -> Result<PackStageTotals> {
        let prepared = self.prepare_frame(context, operations)?;
        let totals = prepared.stage_totals();
        self.activate_prepared(prepared, prepared.commit_horizon())?;
        self.maintain()
            .map_err(PackStoreError::committed_maintenance)?;
        Ok(totals)
    }

    /// Activates a frame built from borrowed values without an intermediate
    /// owned operation graph.
    pub fn append_built(&mut self, builder: PackFrameBuilder) -> Result<PackStageTotals> {
        let prepared = self.prepare_built_append(builder)?;
        let totals = prepared.stage_totals();
        self.activate_prepared(prepared, prepared.commit_horizon())?;
        self.maintain()
            .map_err(PackStoreError::committed_maintenance)?;
        Ok(totals)
    }

    /// Writes and durably syncs one frame and its immutable level-0 run
    /// without changing the manifest, live read view, epoch, or visible tail.
    ///
    /// The returned token supplies the receipt for an external canonical
    /// marker. Exactly one prepared append may exist at a time; callers must
    /// activate it, or drop and reopen the store to discard the orphan suffix.
    pub fn prepare_frame(
        &mut self,
        context: PackFrameContext,
        operations: &[PackOperation],
    ) -> Result<PreparedAppend> {
        validate_frame_context(context)?;
        ensure!(!operations.is_empty(), "append frame must not be empty");
        let rows = operations.len();
        let rows_u64 = u64::try_from(rows).context("frame row count does not fit u64")?;
        let metadata_bytes = rows_u64
            .checked_mul(FRAME_ROW_METADATA_LEN as u64)
            .context("frame metadata length overflows")?;
        let value_bytes = operations.iter().try_fold(0u64, |total, operation| {
            let len = match &operation.kind {
                PackOpKind::Put(value) => {
                    let _ = u32::try_from(value.len()).context("frame value exceeds u32")?;
                    u64::try_from(value.len()).context("frame value length does not fit u64")?
                }
                PackOpKind::Tombstone => 0,
            };
            total
                .checked_add(len)
                .context("frame value length overflows")
        })?;
        self.ensure_frame_limits(rows_u64, metadata_bytes, value_bytes)?;
        self.ensure_index_memory(rows, rows)?;
        let frame_start = self.prepare_frame_start()?;
        let (metadata, values, entries) = encode_frame_payload(operations)?;
        self.prepare_encoded_append(frame_start, context, rows, metadata, values, entries)
    }

    /// Prepares a frame encoded directly from borrowed values.
    ///
    /// The builder must contain exactly the row count declared when it was
    /// created. This retains the same frame and index formats as
    /// [`Self::prepare_frame`] while avoiding intermediate owned values.
    pub fn prepare_built_append(&mut self, builder: PackFrameBuilder) -> Result<PreparedAppend> {
        ensure!(
            builder.store_instance_id() == self.instance_id,
            "frame builder belongs to another pack-store handle"
        );
        let layout = builder.preflight()?;
        self.ensure_frame_limits(
            u64::try_from(layout.rows).context("frame row count does not fit u64")?,
            layout.metadata_bytes,
            layout.value_bytes,
        )?;
        self.ensure_index_memory(layout.rows, layout.distinct_keys)?;
        let frame_start = self.prepare_frame_start()?;
        let (context, rows, metadata, values, entries) = builder.finish()?;
        self.prepare_encoded_append(frame_start, context, rows, metadata, values, entries)
    }

    fn prepare_frame_start(&self) -> Result<u64> {
        ensure!(
            self.pending_append.is_none(),
            "a prepared append is already awaiting activation"
        );
        let physical_len = self.pack.metadata().context("stat append pack")?.len();
        let visible_len = u64::try_from(self.pack_map.as_slice().len())
            .context("visible pack length does not fit u64")?;
        ensure!(
            physical_len == visible_len,
            "append pack contains an unresolved orphan suffix; reopen before preparing another frame"
        );
        Ok(physical_len)
    }

    fn prepare_encoded_append(
        &mut self,
        frame_start: u64,
        context: PackFrameContext,
        rows: usize,
        metadata: Vec<u8>,
        values: Vec<u8>,
        mut entries: Vec<IndexEntry>,
    ) -> Result<PreparedAppend> {
        validate_frame_context(context)?;
        ensure!(rows > 0, "append frame must not be empty");
        ensure!(entries.len() == rows, "frame index row count mismatch");
        let rows_u64 = u64::try_from(rows).context("frame row count does not fit u64")?;
        let metadata_bytes =
            u64::try_from(metadata.len()).context("metadata length does not fit u64")?;
        let value_bytes = u64::try_from(values.len()).context("value length does not fit u64")?;
        let frame_bytes = self.ensure_frame_limits(rows_u64, metadata_bytes, value_bytes)?;
        let _ = validate_payload_rows(&metadata, &values, rows)?;
        ensure!(
            entries.windows(2).all(|pair| {
                pair[0].key < pair[1].key
                    || (pair[0].key == pair[1].key && pair[0].sequence < pair[1].sequence)
            }),
            "frame index rows are not ordered by key and sequence"
        );
        let distinct_key_count = entries
            .iter()
            .map(|entry| entry.key)
            .fold((0usize, None), |(count, previous), key| {
                (count + usize::from(previous != Some(key)), Some(key))
            })
            .0;
        let prospective = self.ensure_index_memory(rows, distinct_key_count)?;
        let epoch = self.next_epoch;
        let segment_id = PackSegmentId::INITIAL;
        let next_prepare_serial = self
            .next_prepare_serial
            .checked_add(1)
            .context("prepared append serial overflows")?;
        let value_start = frame_start
            .checked_add(FRAME_HEADER_LEN as u64)
            .and_then(|offset| offset.checked_add(metadata_bytes))
            .context("absolute frame value offset overflows")?;
        for entry in &mut entries {
            if entry.tombstone {
                ensure!(
                    entry.segment_id == PackSegmentId::INITIAL
                        && entry.value_offset == 0
                        && entry.value_len == 0,
                    "tombstone index entry carries a value location"
                );
            } else {
                entry.segment_id = segment_id;
                entry.value_offset = value_start
                    .checked_add(entry.value_offset)
                    .context("absolute frame value offset overflows")?;
            }
        }
        let structured = run_structured_bytes(entries.len(), distinct_key_count)?;
        ensure!(
            prospective
                == self
                    .decoded_index_bytes
                    .checked_add(structured)
                    .context("decoded index bytes overflow")?,
            "frame index preflight changed after key materialization"
        );
        let metadata_sha256 = frame_metadata_digest(&metadata);
        let value_sha256 = frame_value_digest(&values);
        let header = encode_frame_header(
            epoch,
            context,
            rows,
            metadata.len(),
            values.len(),
            metadata_sha256,
            value_sha256,
        )?;
        let frame_sha256 = frame_digest(&header);
        let footer = encode_frame_footer(epoch, frame_bytes, frame_sha256)?;

        let write_started = Instant::now();
        self.pack.write_all(&header).context("write frame header")?;
        self.pack
            .write_all(&metadata)
            .context("write frame metadata")?;
        self.pack.write_all(&values).context("write frame values")?;
        self.pack.write_all(&footer).context("write frame footer")?;
        let append_write_ns = duration_ns(write_started.elapsed());
        let pack_len = frame_start
            .checked_add(frame_bytes)
            .context("appended pack length overflows")?;
        let receipt = PackFrameReceipt {
            epoch,
            segment_id,
            frame_start,
            frame_end: pack_len,
            context,
            rows: rows_u64,
            metadata_bytes,
            value_bytes,
            frame_sha256,
        };

        // The append frame is now fully written. Its durable sync and the
        // immutable index's pure CPU build do not touch shared mutable state,
        // so overlap them. All fallible publication steps below remain
        // ordered after this join and before the external marker commit.
        let overlap_started = Instant::now();
        let (pack_sync, index_build) = rayon::join(
            || {
                let started = Instant::now();
                let result = self.pack.sync_data().context("sync append pack frame");
                (result, duration_ns(started.elapsed()))
            },
            || {
                let started = Instant::now();
                let min_key = entries.first().expect("non-empty frame").key;
                let max_key = entries.last().expect("non-empty frame").key;
                let fences = build_fences(&entries);
                let filter =
                    build_blocked_bloom(&entries, epoch).context("build run membership filter");
                let result = filter.and_then(|filter| {
                    encode_index_run(epoch, &entries, &fences, &filter, &min_key, &max_key).map(
                        |(index_bytes, records_sha256)| {
                            (
                                min_key,
                                max_key,
                                fences,
                                filter,
                                index_bytes,
                                records_sha256,
                            )
                        },
                    )
                });
                (result, duration_ns(started.elapsed()))
            },
        );
        let publication_overlap_ns = duration_ns(overlap_started.elapsed());
        let (pack_sync_result, pack_sync_ns) = pack_sync;
        pack_sync_result?;
        let (index_result, index_build_ns) = index_build;
        let (min_key, max_key, fences, filter, index_bytes, records_sha256) = index_result?;
        let final_path = self.runs_dir.join(run_file_name(0, epoch, epoch));
        let temp_path = self.runs_dir.join(format!("run-{epoch:020}.tmp"));
        let index_write_started = Instant::now();
        let mut index_file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .with_context(|| format!("create index run {}", temp_path.display()))?;
        index_file
            .write_all(&index_bytes)
            .with_context(|| format!("write index run {}", temp_path.display()))?;
        let index_write_ns = duration_ns(index_write_started.elapsed());
        let index_sync_started = Instant::now();
        index_file
            .sync_data()
            .with_context(|| format!("sync index run {}", temp_path.display()))?;
        let index_sync_ns = duration_ns(index_sync_started.elapsed());
        drop(index_file);
        fs::rename(&temp_path, &final_path).with_context(|| {
            format!(
                "publish index run {} as {}",
                temp_path.display(),
                final_path.display()
            )
        })?;
        let directory_sync_started = Instant::now();
        sync_directory(&self.runs_dir)?;
        let directory_sync_ns = duration_ns(directory_sync_started.elapsed());

        let file = File::open(&final_path)
            .with_context(|| format!("open published index run {}", final_path.display()))?;
        let file_bytes = u64::try_from(index_bytes.len()).context("index bytes do not fit u64")?;
        let map = Mmap::map(&file, file_bytes, &final_path)?;
        drop(file);
        let bloom_offset = INDEX_HEADER_LEN + fences.len() * FENCE_KEY_BYTES;
        let records_offset = u64::try_from(bloom_offset + filter.as_bytes().len())
            .context("index records offset does not fit u64")?;
        let run = LiveRun {
            run: Arc::new(IndexRun {
                format_version: PACK_INDEX_FORMAT_VERSION,
                epoch,
                record_count: u64::try_from(entries.len())
                    .context("index count does not fit u64")?,
                map,
                // Pending runs are not read-visible. `validate_prepared`
                // creates the advised map before external marker commit.
                lookup_map: None,
                records_offset,
                file_bytes,
                min_key,
                max_key,
                min_prefix: key_prefix(&min_key),
                max_prefix: key_prefix(&max_key),
                fences,
                filter: RunFilter {
                    seed: filter.seed(),
                    probes: BLOOM_HASH_PROBES,
                    offset: u64::try_from(bloom_offset).context("Bloom offset does not fit u64")?,
                    bytes: u64::try_from(filter.as_bytes().len())
                        .context("Bloom length does not fit u64")?,
                },
                records_sha256,
                structure_sha256: index_bytes
                    [INDEX_STRUCTURE_SHA256_START..INDEX_STRUCTURE_SHA256_END]
                    .try_into()
                    .expect("structure checksum"),
                memory_bytes: structured,
            }),
            level: 0,
            min_epoch: epoch,
            max_epoch: epoch,
        };
        let stage_totals = PackStageTotals {
            append_write_ns,
            pack_sync_ns,
            index_build_ns,
            publication_overlap_ns,
            index_write_ns,
            index_sync_ns,
            directory_sync_ns,
            frames: 1,
            index_entries: u64::try_from(rows).context("operation count does not fit u64")?,
        };
        let token = PreparedAppend {
            receipt,
            stage_totals,
            store_instance_id: self.instance_id,
            serial: self.next_prepare_serial,
        };
        self.pending_append = Some(PendingAppend {
            token,
            run,
            decoded_index_bytes: prospective,
        });
        self.next_prepare_serial = next_prepare_serial;
        Ok(token)
    }

    fn ensure_frame_limits(&self, rows: u64, metadata_bytes: u64, value_bytes: u64) -> Result<u64> {
        if rows > self.config.max_frame_rows() {
            return Err(PackStoreError::LimitExceeded {
                limit: PackStoreLimit::FrameRows,
                actual: rows,
                maximum: self.config.max_frame_rows(),
            }
            .into());
        }
        let expected_metadata = rows
            .checked_mul(FRAME_ROW_METADATA_LEN as u64)
            .context("frame metadata length overflows")?;
        ensure!(
            rows > 0 && metadata_bytes == expected_metadata,
            "frame metadata length does not match its row count"
        );
        let payload_bytes = metadata_bytes
            .checked_add(value_bytes)
            .context("frame payload length overflows")?;
        if payload_bytes > self.config.max_frame_payload_bytes() {
            return Err(PackStoreError::LimitExceeded {
                limit: PackStoreLimit::FramePayloadBytes,
                actual: payload_bytes,
                maximum: self.config.max_frame_payload_bytes(),
            }
            .into());
        }
        let frame_bytes = payload_bytes
            .checked_add(FRAME_HEADER_LEN as u64)
            .and_then(|bytes| bytes.checked_add(FRAME_FOOTER_LEN as u64))
            .context("encoded frame length overflows")?;
        if frame_bytes > self.config.max_pending_bytes() {
            return Err(PackStoreError::LimitExceeded {
                limit: PackStoreLimit::PendingBytes,
                actual: frame_bytes,
                maximum: self.config.max_pending_bytes(),
            }
            .into());
        }
        Ok(frame_bytes)
    }

    fn ensure_index_memory(&self, rows: usize, distinct_keys: usize) -> Result<u64> {
        ensure!(
            distinct_keys <= rows,
            "distinct key count exceeds row count"
        );
        let structured = run_structured_bytes(rows, distinct_keys)?;
        let prospective = self
            .decoded_index_bytes
            .checked_add(structured)
            .context("decoded index bytes overflow")?;
        if prospective > self.config.max_index_memory_bytes() {
            return Err(PackStoreError::LimitExceeded {
                limit: PackStoreLimit::IndexMemoryBytes,
                actual: prospective,
                maximum: self.config.max_index_memory_bytes(),
            }
            .into());
        }
        Ok(prospective)
    }

    /// Completes every fallible pack operation before an external canonical
    /// marker commit.
    ///
    /// The frame and run are revalidated, the next manifest is durably
    /// published, and its read snapshot is pinned before this method returns.
    /// That manifest and this store handle's current generation are
    /// provisional until the caller commits [`SealedAppend::commit_horizon`].
    /// After a successful external commit, consuming the sealed handoff for a
    /// pointer swap performs no I/O, validation, locking, or allocation.
    ///
    /// If the external commit fails, callers must not append again through
    /// this handle. Drop it and reopen through the preceding canonical horizon;
    /// recovery will discard the sealed suffix and its provisional manifest.
    pub fn seal_prepared(&mut self, prepared: PreparedAppend) -> Result<SealedAppend> {
        let commit_horizon = prepared.commit_horizon();
        let validated = self.validate_prepared(prepared, commit_horizon)?;
        let snapshot = Arc::new(self.pin_snapshot_parts(
            validated.generation,
            &validated.runs,
            &validated.ranges,
            &validated.pack_map,
            validated.lookup_pack_map.as_ref(),
        )?);
        manifest::publish_manifest(&self.root, &validated.manifest)?;
        self.install_validated_append(validated);
        Ok(SealedAppend {
            commit_horizon,
            snapshot,
        })
    }

    /// Activates a prepared append after the caller has durably committed the
    /// matching external marker.
    ///
    /// Shadow validation uses this post-marker path. Authoritative
    /// coordination uses [`Self::seal_prepared`] so every fallible pack
    /// operation completes before the marker commit.
    pub fn activate_prepared(
        &mut self,
        prepared: PreparedAppend,
        committed: PackCommitHorizon,
    ) -> Result<()> {
        let validated = self.validate_prepared(prepared, committed)?;
        manifest::publish_manifest(&self.root, &validated.manifest)?;
        self.install_validated_append(validated);
        Ok(())
    }

    /// Revalidates the pending durable frame and run and constructs the next
    /// immutable generation without publishing it.
    fn validate_prepared(
        &self,
        prepared: PreparedAppend,
        committed: PackCommitHorizon,
    ) -> Result<ValidatedAppend> {
        ensure!(
            prepared.store_instance_id == self.instance_id,
            "prepared append belongs to another pack-store handle"
        );
        let pending = self
            .pending_append
            .as_ref()
            .context("no prepared append is awaiting activation")?;
        ensure!(
            pending.token.serial == prepared.serial
                && pending.token.store_instance_id == prepared.store_instance_id
                && pending.token.receipt == prepared.receipt,
            "prepared append token does not match the pending frame"
        );
        ensure!(
            committed.epoch == prepared.receipt.epoch,
            "external commit marker epoch does not match the prepared frame"
        );
        ensure!(
            committed.segment_id == prepared.receipt.segment_id,
            "external commit marker segment does not match the prepared frame"
        );
        ensure!(
            committed.frame_end == prepared.receipt.frame_end,
            "external commit marker end does not match the prepared frame"
        );
        ensure!(
            committed.context == prepared.receipt.context,
            "external commit marker context does not match the prepared frame"
        );
        ensure!(
            committed.frame_sha256 == prepared.receipt.frame_sha256,
            "external commit marker digest does not match the prepared frame"
        );
        ensure!(
            prepared.receipt.epoch == self.next_epoch,
            "prepared frame activation is out of order"
        );
        ensure!(
            prepared.receipt.segment_id == PackSegmentId::INITIAL,
            "prepared frame names an unavailable segment"
        );
        let next_epoch = self
            .next_epoch
            .checked_add(1)
            .context("append epoch overflows")?;
        let generation = self
            .generation
            .checked_add(1)
            .context("manifest generation overflows")?;
        let physical_len = self
            .pack
            .metadata()
            .context("stat prepared append pack")?
            .len();
        ensure!(
            physical_len == prepared.receipt.frame_end,
            "prepared frame is not the physical pack tail"
        );
        let expected_frame_start = self
            .last_frame_receipt
            .map_or(SEGMENT_HEADER_LEN as u64, |receipt| receipt.frame_end);
        ensure!(
            prepared.receipt.frame_start == expected_frame_start,
            "prepared frame does not continue the committed pack tail"
        );
        let actual_receipt = read_frame_receipt_at(
            &self.pack,
            PackSegmentId::INITIAL,
            prepared.receipt.epoch,
            prepared.receipt.frame_start,
            prepared.receipt.frame_end,
        )?;
        ensure!(
            actual_receipt == prepared.receipt,
            "prepared frame receipt no longer matches durable bytes"
        );
        let pack_map = Arc::new(Mmap::map(
            &self.pack,
            prepared.receipt.frame_end,
            &self.pack_path,
        )?);
        let lookup_pack_map = map_random_if_enabled(
            &self.pack,
            prepared.receipt.frame_end,
            &self.pack_path,
            self.config.read_options(),
        )?
        .map(Arc::new);
        verify_frame(
            &pack_map,
            PackSegmentId::INITIAL,
            prepared.receipt.frame_start,
            prepared.receipt.frame_end,
            prepared.receipt.epoch,
        )?;

        let prepared_run = &pending.run.run;
        let run_path = self.runs_dir.join(run_file_name(
            0,
            prepared.receipt.epoch,
            prepared.receipt.epoch,
        ));
        let verified_run = read_index_run_with_options(&run_path, self.config.read_options())?;
        verify_run(&verified_run)?;
        ensure!(
            verified_run.epoch == prepared_run.epoch
                && verified_run.record_count == prepared_run.record_count
                && verified_run.records_sha256 == prepared_run.records_sha256
                && verified_run.structure_sha256 == prepared_run.structure_sha256
                && verified_run.file_bytes == prepared_run.file_bytes
                && verified_run.records_offset == prepared_run.records_offset
                && verified_run.min_key == prepared_run.min_key
                && verified_run.max_key == prepared_run.max_key
                && verified_run.memory_bytes == prepared_run.memory_bytes,
            "prepared index run no longer matches its durable receipt"
        );
        let live_run = LiveRun {
            run: Arc::new(verified_run),
            level: 0,
            min_epoch: prepared.receipt.epoch,
            max_epoch: prepared.receipt.epoch,
        };
        let mut activated_runs = self.runs.clone();
        activated_runs.push(live_run);
        let mut activated_ranges = self.ranges.clone();
        activated_ranges.push(RunRange {
            min_prefix: activated_runs.last().expect("appended run").run.min_prefix,
            max_prefix: activated_runs.last().expect("appended run").run.max_prefix,
        });
        let entries = activated_runs.iter().map(manifest_entry_of).collect();
        let mut extents = self.extents.clone();
        manifest::append_frame_extent(&mut extents, prepared.receipt)?;
        Ok(ValidatedAppend {
            receipt: prepared.receipt,
            stage_totals: prepared.stage_totals(),
            pack_map,
            lookup_pack_map,
            runs: activated_runs,
            ranges: activated_ranges,
            decoded_index_bytes: pending.decoded_index_bytes,
            next_epoch,
            generation,
            extents: extents.clone(),
            manifest: Manifest {
                generation,
                extents,
                entries,
            },
        })
    }

    /// Exposes a generation after its manifest is durably published. This
    /// method is intentionally infallible so a successful seal cannot leave
    /// disk publication ahead of the writer's in-process bookkeeping.
    fn install_validated_append(&mut self, validated: ValidatedAppend) {
        self.stage_totals.merge(validated.stage_totals);
        self.logical_payload_bytes = self.logical_payload_bytes.saturating_add(
            validated
                .receipt
                .metadata_bytes
                .saturating_add(validated.receipt.value_bytes),
        );
        self.pack_map = validated.pack_map;
        self.lookup_pack_map = validated.lookup_pack_map;
        self.runs = validated.runs;
        *self.level_run_counts.entry(0).or_default() += 1;
        self.ranges = validated.ranges;
        self.decoded_index_bytes = validated.decoded_index_bytes;
        self.next_epoch = validated.next_epoch;
        self.generation = validated.generation;
        self.extents = validated.extents;
        self.last_frame_receipt = Some(validated.receipt);
        self.pending_append = None;
        self.note_peak();
    }
}
