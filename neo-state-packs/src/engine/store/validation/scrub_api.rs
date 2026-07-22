use super::segment::SEGMENT_HEADER_LEN;
use super::*;

impl PackStore {
    pub(super) fn committed_pack_bytes(&self) -> u64 {
        self.last_frame_receipt
            .map_or(0, |receipt| receipt.frame_end)
    }

    /// Re-hashes and structurally decodes every committed frame.
    ///
    /// Normal open verifies frame headers, all derived-run checksums, and the
    /// committed tail payload. This slower operation is the explicit migration
    /// and offline-scrub gate for proving the complete payload prefix rather
    /// than only the tail.
    pub fn scrub_committed_frames(&self) -> Result<PackScrubStats> {
        self.scrub_committed_frames_with(|_, _, _| Ok(()))
    }

    /// Re-hashes and structurally validates every record in every live index
    /// run, including order, fences, and filter no-false-negative behavior.
    /// Dedicated sequential mappings release consumed pages as the scrub
    /// advances, so this offline gate does not retain the full index working
    /// set or perturb the point-read mappings.
    pub fn scrub_index_runs(&self) -> Result<PackIndexScrubStats> {
        let mut stats = PackIndexScrubStats::default();
        let committed_pack_bytes = self.committed_pack_bytes();
        for live in &self.runs {
            scrub_live_index_run(live, &self.runs_dir, committed_pack_bytes)?;
            stats.runs = stats.runs.saturating_add(1);
            match live.run.format_version {
                XOR_INDEX_RUN_FORMAT_VERSION => {
                    stats.v3_runs = stats.v3_runs.saturating_add(1);
                }
                PACK_INDEX_RUN_FORMAT_VERSION => {
                    stats.v4_runs = stats.v4_runs.saturating_add(1);
                }
                _ => unreachable!("reader accepted only supported run versions"),
            }
            stats.records = stats.records.saturating_add(live.run.record_count);
            stats.record_bytes = stats.record_bytes.saturating_add(
                live.run
                    .record_count
                    .saturating_mul(INDEX_RECORD_LEN as u64),
            );
        }
        Ok(stats)
    }

    /// Scrubs and hashes a complete ordered, unique, put-only checkpoint.
    ///
    /// This deliberately rejects runtime version streams containing repeated
    /// keys or tombstones. It proves that checkpoint frame rows reproduce the
    /// same canonical namespace stream hashed by the offline builder.
    pub fn scrub_checkpoint_namespace(&self) -> Result<CheckpointNamespaceEvidence> {
        let mut hasher = Sha256::new();
        hasher.update(CHECKPOINT_NAMESPACE_DIGEST_DOMAIN);
        let mut previous_key = None;
        let scrub = self.scrub_committed_frames_with(|key, kind, value| {
            ensure!(kind == 1, "checkpoint namespace contains a tombstone");
            if let Some(previous) = previous_key {
                ensure!(
                    previous < *key,
                    "checkpoint namespace keys are not strictly increasing"
                );
            }
            hasher.update((PACK_KEY_BYTES as u32).to_le_bytes());
            hasher.update(key);
            hasher.update((value.len() as u64).to_le_bytes());
            hasher.update(value);
            previous_key = Some(*key);
            Ok(())
        })?;
        Ok(CheckpointNamespaceEvidence {
            scrub,
            sha256: hasher.finalize().into(),
        })
    }

    pub(super) fn scrub_committed_frames_with<F>(&self, mut visit: F) -> Result<PackScrubStats>
    where
        F: FnMut(&[u8; PACK_KEY_BYTES], u8, &[u8]) -> Result<()>,
    {
        let committed_bytes = self.committed_pack_bytes();
        let mapping = Mmap::map_sequential(&self.pack, committed_bytes, &self.pack_path)?;
        let bytes = mapping.as_slice();
        let mut stats = PackScrubStats::default();
        let mut offset = SEGMENT_HEADER_LEN;
        let mut release_start = SEGMENT_HEADER_LEN;
        let expected_frames = self
            .last_frame_receipt
            .map_or(0, |receipt| receipt.epoch.saturating_add(1));

        while stats.frames < expected_frames {
            let header_end = offset
                .checked_add(FRAME_HEADER_LEN)
                .context("scrub frame header offset overflows")?;
            let header_bytes: &[u8; FRAME_HEADER_LEN] = bytes
                .get(offset..header_end)
                .with_context(|| format!("committed frame {} header is truncated", stats.frames))?
                .try_into()
                .expect("frame header length");
            let header = validate_frame_header(header_bytes, stats.frames)?;
            let metadata_len = usize::try_from(header.metadata_bytes)
                .context("scrub metadata length does not fit usize")?;
            let metadata_end = header_end
                .checked_add(metadata_len)
                .context("scrub metadata end overflows")?;
            let value_len = usize::try_from(header.value_bytes)
                .context("scrub value length does not fit usize")?;
            let value_end = metadata_end
                .checked_add(value_len)
                .context("scrub value end overflows")?;
            let frame_end = offset
                .checked_add(
                    usize::try_from(header.frame_bytes)
                        .context("scrub frame length does not fit usize")?,
                )
                .context("scrub frame end overflows")?;
            ensure!(
                value_end.checked_add(FRAME_FOOTER_LEN) == Some(frame_end),
                "committed frame {} section lengths do not reach its exact end",
                stats.frames
            );
            let metadata = bytes.get(header_end..metadata_end).with_context(|| {
                format!("committed frame {} metadata is truncated", stats.frames)
            })?;
            let values = bytes.get(metadata_end..value_end).with_context(|| {
                format!("committed frame {} values are truncated", stats.frames)
            })?;
            let footer: &[u8; FRAME_FOOTER_LEN] = bytes
                .get(value_end..frame_end)
                .with_context(|| format!("committed frame {} footer is truncated", stats.frames))?
                .try_into()
                .context("committed frame footer length mismatch")?;
            let expected_rows =
                usize::try_from(header.rows).context("scrub row count does not fit usize")?;
            let mut metadata_hasher = Sha256::new();
            metadata_hasher.update(FRAME_METADATA_DIGEST_DOMAIN);
            let mut value_hasher = Sha256::new();
            value_hasher.update(FRAME_VALUE_DIGEST_DOMAIN);
            let mut metadata_release_start = header_end;
            let mut value_release_start = metadata_end;
            let payload_stats = validate_payload_rows_with_progress(
                metadata,
                values,
                expected_rows,
                &mut visit,
                &mut |section, chunk, consumed| {
                    let (hasher, section_start, section_release_start) = match section {
                        FramePayloadSection::Metadata => (
                            &mut metadata_hasher,
                            header_end,
                            &mut metadata_release_start,
                        ),
                        FramePayloadSection::Values => {
                            (&mut value_hasher, metadata_end, &mut value_release_start)
                        }
                    };
                    hasher.update(chunk);
                    let absolute_end = section_start
                        .checked_add(consumed)
                        .context("scrub section release offset overflows")?;
                    *section_release_start =
                        mapping.advise_dontneed(*section_release_start, absolute_end)?;
                    Ok(())
                },
            )?;
            let metadata_sha256: [u8; 32] = metadata_hasher.finalize().into();
            ensure!(
                metadata_sha256 == header.metadata_sha256,
                "committed frame {} metadata checksum mismatch",
                stats.frames
            );
            let value_sha256: [u8; 32] = value_hasher.finalize().into();
            ensure!(
                value_sha256 == header.value_sha256,
                "committed frame {} value checksum mismatch",
                stats.frames
            );
            validate_frame_footer(footer, header, frame_digest(header_bytes))?;
            stats.frames = stats.frames.saturating_add(1);
            stats.rows = stats.rows.saturating_add(payload_stats.rows);
            stats.puts = stats.puts.saturating_add(payload_stats.puts);
            stats.tombstones = stats.tombstones.saturating_add(payload_stats.tombstones);
            stats.payload_bytes = stats
                .payload_bytes
                .saturating_add(header.metadata_bytes.saturating_add(header.value_bytes));
            stats.value_bytes = stats.value_bytes.saturating_add(payload_stats.value_bytes);
            offset = frame_end;
            release_start = mapping.advise_dontneed(release_start, offset)?;
        }

        ensure!(
            offset == bytes.len(),
            "committed frame prefix ends at {offset}, but mapped pack has {} bytes",
            bytes.len()
        );
        let _ = mapping.advise_dontneed(release_start, bytes.len())?;
        Ok(stats)
    }
}
