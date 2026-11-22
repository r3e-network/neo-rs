use super::checkpoint::*;
use super::logs::*;
use crate::import_batching::{BlockMeta, DrainTrigger, ImportDrainPolicy};
use crate::import_telemetry::ImportCounters;
use anyhow::{Context, Result, bail};
use neo_io::{MemoryReader, Serializable};
use neo_native_contracts::native::LedgerContract;
use neo_primitives::UInt256;
use neo_protocol::payloads::block::Block;
use neo_system::NeoSystem;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::sync::Arc;
use std::time::Instant;
use tracing::info;

/// One decoded archive record that the chain has not accepted yet.
pub(super) struct DecodedBlock {
    pub(super) meta: BlockMeta,
    pub(super) block: Block,
}

/// What one successful persistence unit accepted.
pub(super) struct UnitOutcome {
    pub(super) accepted: u64,
    pub(super) height: u32,
    pub(super) hash: UInt256,
}
/// Mutable state of one `.acc` replay.
///
/// The three frontiers are explicit:
///
/// - **decoded**: `current_height + pending.len()`, the height of the newest
///   record read ahead of the chain,
/// - **accepted**: `current_height`, advanced only when a persistence unit
///   returns successfully,
/// - **verified-durable**: `last_verified_checkpoint`, advanced only by a checked
///   checkpoint flush.
///
/// `imported` therefore counts accepted blocks, never decoded ones, so a failed
/// run cannot report a height it did not commit.
pub(super) struct ImportRun<'a> {
    pub(super) system: &'a Arc<NeoSystem>,
    pub(super) storage_path: Option<&'a str>,
    pub(super) started_at: Instant,
    pub(super) policy: ImportDrainPolicy,
    pub(super) stop_height: Option<u32>,
    pub(super) current_height: u32,
    pub(super) imported: u64,
    pub(super) skipped: u64,
    pub(super) counters: ImportCounters,
    pub(super) last_accepted_hash: UInt256,
    pub(super) chain_parent: Option<UInt256>,
    pub(super) last_verified_checkpoint: Option<PersistedCheckpoint>,
    pub(super) pending: Vec<DecodedBlock>,
    pub(super) empty_block_batches: u64,
    pub(super) last_drain: Option<DrainTrigger>,
    pub(super) progress_window_started_at: Instant,
    pub(super) progress_window_height: u32,
    pub(super) progress_window_counters: ImportCounters,
    pub(super) progress_emissions: u64,
}

impl<'a> ImportRun<'a> {
    pub(super) fn new(
        system: &'a Arc<NeoSystem>,
        storage_path: Option<&'a str>,
        started_at: Instant,
        policy: ImportDrainPolicy,
        stop_height: Option<u32>,
    ) -> Self {
        // The parent of the first decoded block is the tip this process starts
        // from. It is only unknown for a store that has no current block, in
        // which case the header validator stays responsible for that block
        // exactly as it is on the single-block path.
        let accepted_hash = read_local_checkpoint(system).hash;
        let chain_parent = (accepted_hash != UInt256::default()).then_some(accepted_hash);
        let current_height = system.current_block_index();
        Self {
            system,
            storage_path,
            started_at,
            policy,
            stop_height,
            current_height,
            imported: 0,
            skipped: 0,
            counters: ImportCounters::default(),
            last_accepted_hash: accepted_hash,
            chain_parent,
            last_verified_checkpoint: None,
            pending: Vec::new(),
            empty_block_batches: 0,
            last_drain: None,
            progress_window_started_at: started_at,
            progress_window_height: current_height,
            progress_window_counters: ImportCounters::default(),
            progress_emissions: 0,
        }
    }

    /// Height of the newest decoded record, including pending read-ahead.
    pub(super) fn decoded_height(&self) -> u32 {
        let pending = u32::try_from(self.pending.len()).unwrap_or(u32::MAX);
        self.current_height.saturating_add(pending)
    }

    /// Height the next decoded record must carry.
    pub(super) fn next_decoded_height(&self) -> Result<u32> {
        let pending = u32::try_from(self.pending.len()).unwrap_or(u32::MAX);
        self.current_height
            .checked_add(pending)
            .and_then(|height| height.checked_add(1))
            .ok_or_else(|| {
                anyhow::anyhow!("import height overflow: the archive exceeds u32::MAX blocks")
            })
    }

    pub(super) fn last_pending_height(&self) -> u32 {
        self.pending
            .last()
            .map_or(self.current_height, |decoded| decoded.meta.index)
    }

    pub(super) fn heartbeat_due(&self) -> bool {
        self.progress_window_started_at.elapsed().as_secs() >= self.policy.progress_heartbeat_secs
    }

    /// Persists the decoded run. Returns whether the import must stop.
    pub(super) fn drain(&mut self, trigger: DrainTrigger) -> Result<bool> {
        if self.pending.is_empty() {
            return Ok(false);
        }
        let run = std::mem::take(&mut self.pending);
        let len = run.len();
        let outcome = if len == 1 {
            let decoded = run
                .into_iter()
                .next()
                .expect("a run of one block holds one block");
            persist_single_block(self.system, self.chain_parent, decoded).with_context(|| {
                format!(
                    "failed to persist the decoded empty block ({})",
                    trigger.as_str()
                )
            })?
        } else {
            persist_empty_run(self.system, self.chain_parent, run, trigger).with_context(|| {
                format!(
                    "failed to persist a decoded run of {len} empty blocks ({})",
                    trigger.as_str()
                )
            })?
        };
        self.accept(outcome);
        if len >= 2 {
            self.counters.record_empty_batch(len);
            self.empty_block_batches = self.empty_block_batches.saturating_add(1);
        } else {
            self.counters.record_block(0);
        }
        self.last_drain = Some(trigger);
        self.after_unit()
    }

    /// Persists one transaction-bearing block. Returns whether the import must stop.
    pub(super) fn persist_one(&mut self, decoded: DecodedBlock) -> Result<bool> {
        let transaction_count = decoded.meta.transaction_count;
        let outcome = persist_single_block(self.system, self.chain_parent, decoded)?;
        self.accept(outcome);
        self.counters.record_block(transaction_count);
        self.last_drain = None;
        self.after_unit()
    }

    /// Advances the accepted frontier after a successful persistence unit.
    pub(super) fn accept(&mut self, outcome: UnitOutcome) {
        self.current_height = outcome.height;
        self.last_accepted_hash = outcome.hash;
        self.chain_parent = Some(outcome.hash);
        self.imported = self.imported.saturating_add(outcome.accepted);
    }

    /// Runs the checkpoint, stop-height and progress steps of an accepted unit.
    pub(super) fn after_unit(&mut self) -> Result<bool> {
        let periodic_flush_due = self.imported.is_multiple_of(self.policy.flush_interval);
        if periodic_flush_due {
            self.checkpoint("periodic")?;
        }

        if self
            .stop_height
            .is_some_and(|limit| self.current_height >= limit)
        {
            if !periodic_flush_due {
                self.checkpoint("stop-height")?;
            }
            info!(
                target: "neo",
                imported = self.imported,
                current_height = self.current_height,
                stop_height = ?self.stop_height,
                "reached NEO_IMPORT_STOP_HEIGHT; ending import early"
            );
            return Ok(true);
        }

        if self.imported.is_multiple_of(self.policy.progress_interval) || self.heartbeat_due() {
            self.emit_progress();
        }
        Ok(false)
    }

    pub(super) fn checkpoint(&mut self, reason: &str) -> Result<()> {
        self.last_verified_checkpoint = Some(flush_and_verify_checkpoint(
            self.system,
            self.storage_path,
            self.current_height,
            &self.last_accepted_hash,
            self.imported,
            self.skipped,
            reason,
        )?);
        Ok(())
    }

    pub(super) fn emit_progress(&mut self) {
        let window_elapsed = self.progress_window_started_at.elapsed();
        let run_elapsed = self.started_at.elapsed();
        let window = self.counters.delta_since(self.progress_window_counters);
        let window_height_delta =
            u64::from(self.current_height).saturating_sub(u64::from(self.progress_window_height));
        let local_view_height = read_local_checkpoint(self.system).height;
        let current_hash_written = LedgerContract::new()
            .get_block_hash_by_index(&self.system.context().store_cache(), self.current_height)
            .ok()
            .flatten()
            .is_some();
        // Level-0 file count is the cheapest signal that background compaction
        // is keeping up with the import write rate.
        let level_files = self.system.store().level_file_counts();
        let l0_files = level_files
            .iter()
            .find_map(|(level, count)| (*level == 0).then_some(*count))
            .unwrap_or(0);
        self.progress_emissions += 1;
        let decoded_height = self.decoded_height();
        // `rate_blocks_per_sec` keeps its historical meaning (lifetime average
        // for this run); `window_rate_blocks_per_sec` is the rate since the
        // previous emission only, so a stalled or speeding import is visible
        // instead of being averaged away.
        info!(
            target: "neo",
            imported = self.imported,
            skipped = self.skipped,
            current_height = self.current_height,
            decoded_height,
            local_view_height,
            current_hash_written,
            rate_blocks_per_sec = self.counters.blocks_per_sec(run_elapsed),
            window_rate_blocks_per_sec = window.blocks_per_sec(window_elapsed),
            window_secs = window_elapsed.as_secs_f64(),
            window_height_delta,
            window_imported_blocks = window.imported_blocks,
            window_empty_blocks = window.empty_blocks,
            window_non_empty_blocks = window.non_empty_blocks,
            window_transactions = window.transactions,
            window_empty_block_batches = window.empty_block_batches,
            window_batched_empty_blocks = window.batched_empty_blocks,
            imported_blocks = self.counters.imported_blocks,
            empty_blocks = self.counters.empty_blocks,
            non_empty_blocks = self.counters.non_empty_blocks,
            transactions = self.counters.transactions,
            empty_block_batches = self.counters.empty_block_batches,
            batched_empty_blocks = self.counters.batched_empty_blocks,
            batching_enabled = self.policy.batching_enabled(),
            empty_batch_cap = self.policy.empty_batch_cap,
            drain_trigger = self.last_drain.map(DrainTrigger::as_str),
            progress_emissions = self.progress_emissions,
            l0_files,
            level_files = ?level_files,
            "acc import progress"
        );
        self.progress_window_started_at = Instant::now();
        self.progress_window_height = self.current_height;
        self.progress_window_counters = self.counters;
    }
}

/// Reads and decodes one archive record.
///
/// Returns `None` for a record the local chain already holds, which is how an
/// indexed resume skips persisted heights without decoding them.
pub(super) fn read_decoded_block(
    reader: &mut BufReader<File>,
    payload: &mut Vec<u8>,
    size_buf: &mut [u8; 4],
    declared_start: u32,
    item: u32,
    run: &mut ImportRun<'_>,
) -> Result<Option<DecodedBlock>> {
    reader
        .read_exact(&mut size_buf[..])
        .with_context(|| format!("failed to read block size at item {item}"))?;
    let block_size = u32::from_le_bytes(*size_buf) as usize;
    if block_size == 0 {
        bail!("encountered zero-sized block payload at item {item}");
    }

    let Some(declared_index) = declared_start.checked_add(item) else {
        bail!("declared block index overflow at item {item}");
    };

    if declared_index <= run.current_height {
        let skip_len = i64::try_from(block_size)
            .with_context(|| format!("block size too large to skip at item {item}"))?;
        reader
            .seek(SeekFrom::Current(skip_len))
            .with_context(|| format!("failed to seek past block payload at item {item}"))?;
        run.skipped += 1;
        if run.skipped.is_multiple_of(100_000) {
            info!(
                target: "neo",
                skipped = run.skipped,
                current_height = run.current_height,
                "acc import scan progress"
            );
        }
        return Ok(None);
    }

    payload.resize(block_size, 0u8);
    reader
        .read_exact(&mut payload[..block_size])
        .with_context(|| format!("failed to read block payload at item {item}"))?;

    let mut block_reader = MemoryReader::new(&payload[..block_size]);
    let mut block = <Block as Serializable>::deserialize(&mut block_reader)
        .with_context(|| format!("failed to deserialize block payload at item {item}"))?;
    let block_index = block.index();

    if block_index != declared_index {
        bail!(
            "import block index mismatch at item {item}: declared {declared_index}, decoded {block_index}"
        );
    }

    // The sequence check runs against the *decoded* frontier, because blocks that
    // are still pending are read ahead of the chain tip but already consumed
    // their archive slot.
    let expected = run.next_decoded_height()?;
    if block_index != expected {
        bail!("import sequence gap at item {item}: expected block {expected}, got {block_index}");
    }

    // The hash is computed from the decoded header before the block is moved into
    // persistence, so the checkpoint verified later must reproduce exactly this
    // value.
    let transaction_count = block.transactions.len();
    let hash = block
        .try_hash()
        .with_context(|| format!("failed to hash imported block {block_index}"))?;
    let prev_hash = *block.prev_hash();
    Ok(Some(DecodedBlock {
        meta: BlockMeta {
            index: block_index,
            hash,
            prev_hash,
            transaction_count,
        },
        block,
    }))
}
