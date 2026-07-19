use super::*;

impl PackStore {
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
        for live in &self.runs {
            scrub_live_index_run(live, &self.runs_dir)?;
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

    fn scrub_committed_frames_with<F>(&self, mut visit: F) -> Result<PackScrubStats>
    where
        F: FnMut(&[u8; PACK_KEY_BYTES], u8, &[u8]) -> Result<()>,
    {
        let bytes = self.pack_map.as_slice();
        let mut stats = PackScrubStats::default();
        let mut offset = 0usize;
        let expected_frames = self
            .last_frame_receipt
            .map_or(0, |receipt| receipt.epoch.saturating_add(1));

        while stats.frames < expected_frames {
            let header_end = offset
                .checked_add(FRAME_HEADER_LEN)
                .context("scrub frame header offset overflows")?;
            let header: &[u8; FRAME_HEADER_LEN] = bytes
                .get(offset..header_end)
                .with_context(|| format!("committed frame {} header is truncated", stats.frames))?
                .try_into()
                .expect("frame header length");
            let payload_len = validate_frame_header(header, stats.frames)?;
            let payload_len =
                usize::try_from(payload_len).context("scrub payload length does not fit usize")?;
            let payload_end = header_end
                .checked_add(payload_len)
                .context("scrub frame end offset overflows")?;
            let payload = bytes.get(header_end..payload_end).with_context(|| {
                format!("committed frame {} payload is truncated", stats.frames)
            })?;
            ensure!(
                digest(payload).as_slice() == &header[40..72],
                "committed frame {} payload checksum mismatch",
                stats.frames
            );
            let expected_rows = usize::try_from(u64_at(header, 24)?)
                .context("scrub row count does not fit usize")?;
            let payload_stats = validate_payload_rows_with(payload, expected_rows, &mut visit)?;
            stats.frames = stats.frames.saturating_add(1);
            stats.rows = stats.rows.saturating_add(payload_stats.rows);
            stats.puts = stats.puts.saturating_add(payload_stats.puts);
            stats.tombstones = stats.tombstones.saturating_add(payload_stats.tombstones);
            stats.payload_bytes = stats
                .payload_bytes
                .saturating_add(u64::try_from(payload.len()).expect("payload length fits u64"));
            stats.value_bytes = stats.value_bytes.saturating_add(payload_stats.value_bytes);
            offset = payload_end;
        }

        ensure!(
            offset == bytes.len(),
            "committed frame prefix ends at {offset}, but mapped pack has {} bytes",
            bytes.len()
        );
        Ok(stats)
    }
}
