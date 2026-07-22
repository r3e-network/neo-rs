//! Verify shadow or authoritative StateService node packs.
//!
//! Shadow mode compares a reservoir sample of MDBX `0xf0` node rows with the
//! optional shadow prefix. Authority mode validates either a complete,
//! unactivated checkpoint against the frozen MDBX StateService tip or an
//! activated pack against the mandatory MDBX authority marker. It never
//! treats stale MDBX node rows as authoritative after activation.
//!
//! Usage:
//!   neo-pack-verify --mdbx <canonical-store-dir> --pack <shadow-packs-dir>
//!     [--samples N] [--walk-cap N | --full-scan] [--maintain]
//!     [--random-point-mmap]
//!   neo-pack-verify --mode authority --network-magic <u32-or-hex>
//!     --mdbx <canonical-store-dir> --pack <authoritative-packs-dir>
//!     [--samples N] [--walk-cap N | --full-scan]
//!     [--scrub] [--scrub-indexes] [--lookup-digest-samples N]
//!     [--random-point-mmap] [--maintain | --gc]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use neo_state_packs::authority::{AUTHORITATIVE_HIGH_WATER_KEY, AuthoritativeHighWaterRecord};
use neo_state_packs::shadow::{SHADOW_HIGH_WATER_KEY, ShadowHighWaterRecord};
use neo_state_packs::{
    PACK_FRAME_FORMAT_VERSION, PACK_INDEX_FORMAT_VERSION, PACK_KEY_BYTES,
    PACK_MANIFEST_FORMAT_VERSION, PACK_SEGMENT_FORMAT_VERSION, PACK_SEGMENT_HEADER_LEN,
    PackCommitHorizon, PackFrameContext, PackSegmentId, PackStore, PackStoreConfig,
    PackStoreOptions,
};
use neo_state_service::read_current_local_root;
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::storage::StorageConfig;
use neo_storage::persistence::{RawReadOnlyStore, StoreFactory, TransactionalStore};
use serde::Deserialize;

#[path = "neo_pack_verify/evidence.rs"]
mod evidence;
use evidence::{
    gc_and_reopen_with_evidence, print_materialized_evidence, scrub_indexes_with_label,
    validate_authority_mutation_flags,
};
#[path = "neo_pack_verify/checkpoint_compare.rs"]
mod checkpoint_compare;
use checkpoint_compare::{compare_checkpoint_nodes, decode_hash, parse_checkpoint_network_magic};

const STATE_NODE_PREFIX: u8 = 0xf0;
const STATE_SERVICE_NAMESPACE: &str = "neo_state_service";
const CHECKPOINT_SCHEMA_VERSION: u32 = 4;
const BATCH: usize = 1_024;
// Neo MPT nodes are well below this defensive verifier ceiling. Keep a wider
// protocol-independent margin while rejecting corrupt multi-gigabyte index
// lengths before allocation.
const AUTHORITY_LOOKUP_MAX_VALUE_BYTES: usize = 1024 * 1024;
const AUTHORITY_LOOKUP_BATCH_VALUE_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_MAX_INDEX_MEMORY_MB: u64 = 256;
const DEFAULT_WALK_CAP: u64 = 100_000;

fn pack_store_config(
    max_index_memory_mb: u64,
    options: PackStoreOptions,
) -> Result<PackStoreConfig> {
    let max_index_memory_bytes = max_index_memory_mb
        .checked_mul(1024 * 1024)
        .context("--max-index-memory-mb overflows bytes")?;
    PackStoreConfig::default()
        .with_max_index_memory_bytes(max_index_memory_bytes)
        .context("validate pack index-memory bound")?
        .with_read_options(options)
        .context("validate pack read options")
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum VerificationMode {
    #[default]
    Shadow,
    Authority,
}

impl VerificationMode {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "shadow" => Ok(Self::Shadow),
            "authority" => Ok(Self::Authority),
            _ => bail!("--mode must be shadow or authority"),
        }
    }
}

#[derive(Debug, Deserialize)]
struct CheckpointMarker {
    schema_version: u32,
    authoritative_ready: bool,
    complete: bool,
    source_backend: String,
    source_namespace: String,
    network_magic: String,
    source_height: u32,
    source_root_internal_bytes: String,
    source_namespace_sha256: String,
    rows: u64,
    value_bytes: u64,
    frames: u64,
    pack_segment_format_version: u32,
    pack_frame_format_version: u32,
    pack_index_format_version: u32,
    pack_manifest_format_version: u32,
    tip_epoch: u64,
    tip_segment_id: u64,
    tip_frame_end: u64,
    tip_frame_sha256: String,
    scrubbed_frames: u64,
    scrubbed_rows: u64,
    scrubbed_puts: u64,
    scrubbed_tombstones: u64,
    scrubbed_value_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CheckpointBinding {
    source_root: [u8; 32],
    store_identity: [u8; 32],
    tip_frame_sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuthorityState {
    Checkpoint,
    Marker,
}

struct XorShift64(u64);

impl XorShift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

fn main() -> Result<()> {
    // Paths may also come from the environment so invocations can avoid
    // embedding the store path in argv (process-list watchers match argv).
    let mut mdbx_path: Option<PathBuf> = std::env::var("NPV_MDBX").ok().map(PathBuf::from);
    let mut pack_path: Option<PathBuf> = std::env::var("NPV_PACK").ok().map(PathBuf::from);
    let mut mode = VerificationMode::Shadow;
    let mut network_magic = None;
    let mut samples: usize = 100_000;
    let mut walk_cap = DEFAULT_WALK_CAP;
    let mut walk_cap_set = false;
    let mut full_scan = false;
    let mut max_index_memory_mb = DEFAULT_MAX_INDEX_MEMORY_MB;
    let mut maintain = false;
    let mut scrub = false;
    let mut scrub_indexes = false;
    let mut lookup_digest_samples = 0usize;
    let mut random_point_mmap = false;
    let mut gc = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mdbx" => mdbx_path = args.next().map(PathBuf::from),
            "--pack" => pack_path = args.next().map(PathBuf::from),
            "--mode" => {
                mode = VerificationMode::parse(
                    &args.next().context("--mode requires shadow or authority")?,
                )?;
            }
            "--network-magic" => {
                network_magic = Some(parse_u32(
                    &args.next().context("--network-magic requires a number")?,
                )?);
            }
            "--samples" => {
                samples = args
                    .next()
                    .and_then(|value| value.parse::<usize>().ok())
                    .context("--samples requires a number")?
            }
            "--walk-cap" => {
                if full_scan {
                    bail!("--walk-cap conflicts with --full-scan");
                }
                walk_cap = args
                    .next()
                    .and_then(|value| value.parse::<u64>().ok())
                    .context("--walk-cap requires a number")?;
                if walk_cap == 0 {
                    bail!("--walk-cap must be greater than zero; use --full-scan explicitly");
                }
                walk_cap_set = true;
            }
            "--max-index-memory-mb" => {
                max_index_memory_mb = args
                    .next()
                    .and_then(|value| value.parse::<u64>().ok())
                    .context("--max-index-memory-mb requires a number")?;
                if max_index_memory_mb == 0 {
                    bail!("--max-index-memory-mb must be greater than zero");
                }
            }
            "--maintain" => maintain = true,
            "--scrub" => scrub = true,
            "--scrub-indexes" => scrub_indexes = true,
            "--lookup-digest-samples" => {
                lookup_digest_samples = args
                    .next()
                    .and_then(|value| value.parse::<usize>().ok())
                    .context("--lookup-digest-samples requires a number")?;
                if lookup_digest_samples == 0 {
                    bail!("--lookup-digest-samples must be greater than zero");
                }
            }
            "--random-point-mmap" => random_point_mmap = true,
            "--gc" => gc = true,
            "--full-scan" => {
                if walk_cap_set {
                    bail!("--full-scan conflicts with --walk-cap");
                }
                full_scan = true;
            }
            other => bail!("unknown argument {other}"),
        }
    }
    let mdbx_path = mdbx_path.context("--mdbx is required")?;
    let pack_path = pack_path.context("--pack is required")?;
    if mode == VerificationMode::Shadow {
        ensure!(
            network_magic.is_none(),
            "--network-magic is only valid with --mode authority"
        );
        ensure!(!scrub, "--scrub is only valid with --mode authority");
        ensure!(
            !scrub_indexes,
            "--scrub-indexes is only valid with --mode authority"
        );
        ensure!(
            lookup_digest_samples == 0,
            "--lookup-digest-samples is only valid with --mode authority"
        );
        ensure!(!gc, "--gc is only valid with --mode authority");
    } else {
        validate_authority_mutation_flags(maintain, gc, scrub_indexes, lookup_digest_samples)?;
    }

    let canonical: Arc<RuntimeStore> = StoreFactory::get_store_with_config(
        "mdbx",
        StorageConfig {
            path: mdbx_path,
            read_only: true,
            ..Default::default()
        },
    )
    .map_err(|err| anyhow::anyhow!("open MDBX store: {err}"))?;
    let state_store = canonical
        .open_coordinated_namespace(STATE_SERVICE_NAMESPACE)
        .context("open coordinated MDBX StateService namespace")?;

    if mode == VerificationMode::Authority {
        return verify_authority(
            &state_store,
            &pack_path,
            network_magic.context("--network-magic is required with --mode authority")?,
            max_index_memory_mb,
            PackStoreOptions {
                random_point_mmap,
                ..PackStoreOptions::default()
            },
            samples,
            walk_cap,
            full_scan,
            maintain,
            scrub,
            scrub_indexes,
            lookup_digest_samples,
            gc,
        )
        .map(|_| ());
    }

    // High-water marker first: it defines the shadow commit horizon.
    let high_water = match state_store.maintenance_metadata(SHADOW_HIGH_WATER_KEY)? {
        Some(record) => {
            let marker = ShadowHighWaterRecord::decode(&record)
                .context("high-water marker failed to decode")?;
            println!(
                "high-water: epoch={} frames={} node_ops={} value_bytes={} blocks={}..={} root=0x{}",
                marker.epoch,
                marker.frames_total,
                marker.node_operations,
                marker.node_put_value_bytes,
                marker.frame_context.block_start,
                marker.frame_context.block_end,
                hex::encode(marker.frame_context.resulting_root),
            );
            marker
        }
        None => bail!("high-water marker is absent; no shadow pack prefix is canonical"),
    };

    let pack_config = pack_store_config(
        max_index_memory_mb,
        PackStoreOptions {
            random_point_mmap,
            ..PackStoreOptions::default()
        },
    )?;
    let mut pack = PackStore::open_at_commit_horizon(
        &pack_path,
        pack_config,
        Some(high_water.commit_horizon()),
    )
    .context("opening bounded shadow packs at the canonical high-water marker")?;
    let opened = pack.open_validation();
    println!(
        "opened: frames={} runs={} index_entries={}",
        opened.frames, opened.runs, opened.index_entries
    );
    if maintain {
        pack.maintain()
            .context("maintaining derived pack indexes")?;
        let stats = pack.compaction_stats();
        println!(
            "maintained: cycles={} runs_merged={} runs_produced={} input_records={} output_records={} bytes_written={} wall_ns={}",
            stats.cycles,
            stats.runs_merged,
            stats.runs_produced,
            stats.input_records,
            stats.output_records,
            stats.bytes_written,
            stats.wall_ns,
        );
    }

    // Single-pass reservoir sample over the full node-key range so no key
    // list is materialized.
    let prefix = vec![STATE_NODE_PREFIX];
    let mut rng = XorShift64(0x9E37_79B9_7F4A_7C15);
    let mut reservoir: Vec<[u8; 33]> = Vec::with_capacity(samples);
    let mut total_keys = 0u64;
    let maximum = (!full_scan).then_some(walk_cap);
    state_store.visit_raw_keys_with_prefix(&prefix, maximum, |key| {
        if key.len() != 33 {
            return;
        }
        let key: [u8; 33] = key.try_into().expect("33-byte key");
        if (total_keys as usize) < samples {
            reservoir.push(key);
        } else {
            let j = rng.next() % (total_keys + 1);
            if (j as usize) < samples {
                reservoir[j as usize] = key;
            }
        }
        total_keys += 1;
    })?;
    if !full_scan && total_keys >= walk_cap {
        println!("walk capped at {walk_cap} keys (prefix-bounded sample)");
    }
    println!("mdbx node keys: {total_keys}; sampled: {}", reservoir.len());

    reservoir.sort_unstable();
    let (mut matched, mut mismatched, mut absent) = (0u64, 0u64, 0u64);
    let mut first_mismatch: Option<([u8; 33], Vec<u8>, Vec<u8>)> = None;
    for chunk in reservoir.chunks(BATCH) {
        let pack_values = pack.get_many_sorted(chunk)?;
        for (key, pack_value) in chunk.iter().zip(pack_values) {
            let mdbx_value = state_store.try_get_bytes(key.as_slice());
            match (pack_value, mdbx_value) {
                (Some(pack_bytes), Some(mdbx_bytes)) if pack_bytes == mdbx_bytes => {
                    matched += 1;
                }
                (Some(pack_bytes), mdbx_bytes) => {
                    mismatched += 1;
                    if first_mismatch.is_none() {
                        first_mismatch = Some((*key, pack_bytes, mdbx_bytes.unwrap_or_default()));
                    }
                }
                (None, _) => absent += 1,
            }
        }
        pack.reclaim_random_lookup_pages()?;
    }

    println!("matched: {matched}");
    println!("absent-in-pack (predates shadow): {absent}");
    println!("MISMATCHED: {mismatched}");
    if let Some((key, pack_bytes, mdbx_bytes)) = first_mismatch {
        println!(
            "first mismatch at 0x{}: pack {} bytes (0x{}…), mdbx {} bytes (0x{}…)",
            hex::encode(key),
            pack_bytes.len(),
            hex::encode(&pack_bytes[..pack_bytes.len().min(16)]),
            mdbx_bytes.len(),
            hex::encode(&mdbx_bytes[..mdbx_bytes.len().min(16)]),
        );
    }
    if mismatched > 0 {
        std::process::exit(1);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn verify_authority(
    state_store: &RuntimeStore,
    pack_path: &Path,
    network_magic: u32,
    max_index_memory_mb: u64,
    pack_options: PackStoreOptions,
    samples: usize,
    walk_cap: u64,
    full_scan: bool,
    maintain: bool,
    scrub: bool,
    scrub_indexes: bool,
    lookup_digest_samples: usize,
    gc: bool,
) -> Result<AuthorityState> {
    // Keep the mutation contract enforced here as well as in CLI parsing. The
    // verifier is unit-tested through this function directly, and callers
    // embedding the verifier must not be able to skip the evidence gate.
    validate_authority_mutation_flags(maintain, gc, scrub_indexes, lookup_digest_samples)?;
    let checkpoint = read_checkpoint(pack_path)?;
    let binding = validate_checkpoint(&checkpoint, network_magic)?;
    let state_tip = read_state_tip(state_store)?;
    let durable_marker = state_store
        .maintenance_metadata(AUTHORITATIVE_HIGH_WATER_KEY)
        .context("read authoritative pack high-water marker")?;

    let (authority_state, horizon, expected_frame_end, expected_frame_digest) = match durable_marker
    {
        Some(bytes) => {
            let marker = AuthoritativeHighWaterRecord::decode(&bytes)
                .context("decode authoritative pack high-water marker")?;
            marker
                .validate_identity(network_magic, binding.store_identity)
                .context("validate authoritative pack marker identity")?;
            ensure!(
                (marker.block_index, marker.state_root) == state_tip,
                "authoritative marker tip ({}, 0x{}) differs from StateService metadata ({}, 0x{})",
                marker.block_index,
                hex::encode(marker.state_root),
                state_tip.0,
                hex::encode(state_tip.1)
            );
            ensure!(
                marker.block_index >= checkpoint.source_height,
                "authoritative marker predates its base checkpoint"
            );
            ensure!(
                marker.epoch >= checkpoint.tip_epoch,
                "authoritative marker pack horizon predates its base checkpoint"
            );
            println!(
                "authority marker: epoch={} segment={} frame_end={} frame_blocks={}..={} block={} root=0x{} frame_sha256=0x{}",
                marker.epoch,
                marker.segment_id,
                marker.frame_end,
                marker.frame_context.block_start,
                marker.frame_context.block_end,
                marker.block_index,
                hex::encode(marker.state_root),
                hex::encode(marker.frame_sha256),
            );
            (
                AuthorityState::Marker,
                marker.commit_horizon(),
                marker.frame_end,
                marker.frame_sha256,
            )
        }
        None => {
            ensure!(
                state_tip == (checkpoint.source_height, binding.source_root),
                "StateService tip does not equal the unactivated checkpoint base"
            );
            println!(
                "authority checkpoint: unactivated base block={} root=0x{} identity=0x{}",
                checkpoint.source_height,
                hex::encode(binding.source_root),
                hex::encode(binding.store_identity),
            );
            (
                AuthorityState::Checkpoint,
                PackCommitHorizon {
                    epoch: checkpoint.tip_epoch,
                    segment_id: PackSegmentId::new(checkpoint.tip_segment_id),
                    frame_end: checkpoint.tip_frame_end,
                    context: PackFrameContext::new(
                        checkpoint.source_height,
                        checkpoint.source_height,
                        binding.source_root,
                        binding.source_root,
                    ),
                    frame_sha256: binding.tip_frame_sha256,
                },
                checkpoint.tip_frame_end,
                binding.tip_frame_sha256,
            )
        }
    };
    ensure!(
        authority_state != AuthorityState::Checkpoint || full_scan,
        "unactivated checkpoint authority verification requires --full-scan of the independent MDBX namespace"
    );
    ensure!(
        authority_state != AuthorityState::Checkpoint || scrub,
        "unactivated checkpoint authority verification requires --scrub of the complete pack namespace"
    );
    ensure!(
        authority_state != AuthorityState::Checkpoint || scrub_indexes,
        "unactivated checkpoint authority verification requires --scrub-indexes of every derived run"
    );

    let pack_config = pack_store_config(max_index_memory_mb, pack_options)?;
    let mut pack = PackStore::open_at_commit_horizon(pack_path, pack_config, Some(horizon))
        .with_context(|| {
            format!(
                "open authoritative packs at the canonical horizon {}",
                pack_path.display()
            )
        })?;
    let opened = pack.open_validation();
    let receipt = pack
        .last_frame_receipt()
        .context("authoritative pack has no committed frame")?;
    ensure!(
        receipt.epoch == horizon.epoch
            && receipt.segment_id == horizon.segment_id
            && receipt.frame_end == expected_frame_end
            && receipt.context == horizon.context
            && receipt.frame_sha256 == expected_frame_digest,
        "authoritative pack tip differs from its canonical checkpoint/marker"
    );
    if authority_state == AuthorityState::Checkpoint {
        ensure!(
            opened.frames == checkpoint.frames && opened.index_entries == checkpoint.rows,
            "checkpoint pack geometry differs from checkpoint.json"
        );
    }
    println!(
        "opened: frames={} runs={} index_entries={} tip_epoch={} tip_segment={} tip_frame_end={}",
        opened.frames,
        opened.runs,
        opened.index_entries,
        receipt.epoch,
        receipt.segment_id,
        receipt.frame_end,
    );
    let debt = pack.compaction_debt();
    println!(
        "compaction debt: live_runs={} excess_runs={} decoded_index_bytes={} max_index_memory_bytes={} backpressure={}",
        debt.live_runs,
        debt.excess_runs,
        debt.decoded_index_bytes,
        debt.max_index_memory_bytes,
        debt.backpressure_required,
    );
    let metrics = pack.metrics().context("read pack performance metrics")?;
    println!(
        "pack metrics: append_frames={} append_entries={} append_overlap_ns={} point_reads={} point_hits={} point_misses={} sorted_batches={} sorted_keys={} sorted_hits={} logical_payload_bytes={} physical_pack_bytes={} physical_index_bytes={} live_runs={} decoded_index_memory_bytes={}",
        metrics.append.frames,
        metrics.append.index_entries,
        metrics.append.publication_overlap_ns,
        metrics.reads.point_reads,
        metrics.reads.point_hits,
        metrics.reads.point_misses,
        metrics.reads.sorted_batches,
        metrics.reads.sorted_keys,
        metrics.reads.sorted_hits,
        metrics.logical_payload_bytes,
        metrics.physical_pack_bytes,
        metrics.physical_index_bytes,
        metrics.live_runs,
        metrics.decoded_index_memory_bytes,
    );
    if let Some(plan) = pack
        .plan_compaction()
        .context("plan derived index maintenance")?
    {
        println!(
            "compaction plan: estimated_peak_bytes={} max_workspace_bytes={}",
            plan.estimated_workspace_bytes(),
            plan.max_workspace_bytes(),
        );
    }
    if scrub_indexes {
        scrub_indexes_with_label(
            if maintain {
                "pre-maintenance"
            } else if gc {
                "pre-gc"
            } else if authority_state == AuthorityState::Checkpoint {
                "checkpoint"
            } else {
                "current"
            },
            &pack,
        )?;
    }
    ensure_authoritative_root(&pack, state_tip.1)?;
    if authority_state == AuthorityState::Checkpoint {
        compare_checkpoint_nodes(
            state_store,
            &pack,
            &checkpoint,
            binding.store_identity,
            samples,
            walk_cap,
            full_scan,
        )?;
    }
    ensure!(
        read_state_tip(state_store)? == state_tip,
        "StateService tip changed during pre-mutation authority verification"
    );
    let pre_maintenance_evidence = if lookup_digest_samples == 0 {
        None
    } else {
        let evidence = pack
            .materialized_view_evidence(lookup_digest_samples)
            .context("produce pre-maintenance materialized-view evidence")?;
        print_materialized_evidence(
            if maintain {
                "pre-maintenance"
            } else if gc {
                "pre-gc"
            } else {
                "current"
            },
            &evidence,
        );
        Some(evidence)
    };

    if scrub {
        let stats = if authority_state == AuthorityState::Checkpoint {
            let evidence = pack
                .scrub_checkpoint_namespace()
                .context("scrub and hash authoritative checkpoint namespace")?;
            ensure!(
                evidence.sha256 == binding.store_identity,
                "checkpoint pack namespace digest differs from checkpoint.json"
            );
            println!(
                "checkpoint pack digest: 0x{} (all ordered key/value bytes)",
                hex::encode(evidence.sha256)
            );
            evidence.scrub
        } else {
            pack.scrub_committed_frames()
                .context("scrub authoritative committed frame prefix")?
        };
        if authority_state == AuthorityState::Checkpoint {
            ensure!(
                stats.frames == checkpoint.scrubbed_frames
                    && stats.rows == checkpoint.scrubbed_rows
                    && stats.puts == checkpoint.scrubbed_puts
                    && stats.tombstones == checkpoint.scrubbed_tombstones
                    && stats.value_bytes == checkpoint.scrubbed_value_bytes,
                "checkpoint payload scrub differs from checkpoint.json"
            );
        }
        println!(
            "scrubbed: frames={} rows={} puts={} tombstones={} payload_bytes={} value_bytes={}",
            stats.frames,
            stats.rows,
            stats.puts,
            stats.tombstones,
            stats.payload_bytes,
            stats.value_bytes,
        );
    }
    if maintain {
        let before = pre_maintenance_evidence
            .as_ref()
            .context("authority maintenance requires pre-adoption evidence")?;
        let mut candidate_cycles = 0u64;
        while let Some(plan) = pack
            .plan_compaction()
            .context("plan authoritative pack compaction")?
        {
            let prepared = plan
                .build()
                .context("build staged authoritative pack compaction")?;
            let candidate = pack
                .prepared_compaction_evidence(&prepared, lookup_digest_samples)
                .context("produce pre-adoption compaction evidence")?;
            let label = format!("candidate-{}", candidate_cycles + 1);
            print_materialized_evidence(&label, &candidate);
            ensure!(
                before.state_matches(&candidate),
                "staged compaction changes materialized winner or lookup evidence"
            );
            pack.scrub_prepared_compaction(&prepared)?;
            pack.adopt_compaction(prepared)
                .context("publish validated authoritative pack compaction")?;
            candidate_cycles = candidate_cycles.saturating_add(1);
        }
        ensure!(
            candidate_cycles > 0,
            "--maintain found no compaction plan; rerun verification without mutation"
        );
        let stats = pack.compaction_stats();
        ensure!(
            stats.cycles == candidate_cycles,
            "published compaction cycles differ from validated candidate cycles"
        );
        println!(
            "maintained: cycles={} runs_merged={} runs_produced={} input_records={} output_records={} bytes_written={} wall_ns={}",
            stats.cycles,
            stats.runs_merged,
            stats.runs_produced,
            stats.input_records,
            stats.output_records,
            stats.bytes_written,
            stats.wall_ns,
        );
        if let Some(before) = pre_maintenance_evidence {
            let after = pack
                .materialized_view_evidence(lookup_digest_samples)
                .context("produce post-maintenance materialized-view evidence")?;
            print_materialized_evidence("post-maintenance", &after);
            ensure!(
                before.state_matches(&after),
                "materialized winner or lookup evidence changed across maintenance"
            );
            scrub_indexes_with_label("post-maintenance", &pack)?;
            drop(pack);
            pack = PackStore::open_at_commit_horizon(pack_path, pack_config, Some(horizon))
                .context("reopen authoritative packs after maintenance")?;
            let reopened = pack
                .materialized_view_evidence(lookup_digest_samples)
                .context("produce reopened materialized-view evidence")?;
            print_materialized_evidence("reopened", &reopened);
            ensure!(
                after.state_matches(&reopened),
                "materialized winner or lookup evidence changed after reopen"
            );
            let reopened_receipt = pack
                .last_frame_receipt()
                .context("reopened authoritative pack has no committed frame")?;
            ensure!(
                reopened_receipt.epoch == horizon.epoch
                    && reopened_receipt.segment_id == horizon.segment_id
                    && reopened_receipt.frame_end == expected_frame_end
                    && reopened_receipt.context == horizon.context
                    && reopened_receipt.frame_sha256 == expected_frame_digest,
                "reopened authoritative pack tip differs from its canonical checkpoint/marker"
            );
            println!("maintenance equivalence: pre/post/reopen evidence matches exactly");
        }
    }
    if maintain {
        scrub_indexes_with_label("reopened", &pack)?;
    }
    if gc {
        let before = pre_maintenance_evidence.context("GC requires materialized evidence")?;
        pack = gc_and_reopen_with_evidence(
            pack,
            pack_path,
            pack_config,
            horizon,
            lookup_digest_samples,
            &before,
        )?;
    }

    if maintain || gc {
        ensure_authoritative_root(&pack, state_tip.1)?;
    }
    ensure!(
        read_state_tip(state_store)? == state_tip,
        "StateService tip changed during authority verification"
    );
    if authority_state == AuthorityState::Marker {
        println!(
            "MDBX node comparison: skipped after activation; the marker-bound pack is authoritative"
        );
    }
    println!(
        "authority verification: ok ({})",
        match authority_state {
            AuthorityState::Checkpoint => "checkpoint base",
            AuthorityState::Marker => "mandatory marker",
        }
    );
    Ok(authority_state)
}

fn ensure_authoritative_root(pack: &PackStore, root: [u8; 32]) -> Result<()> {
    let mut root_key = [0u8; PACK_KEY_BYTES];
    root_key[0] = STATE_NODE_PREFIX;
    root_key[1..].copy_from_slice(&root);
    ensure!(
        pack.get_bounded(&root_key, AUTHORITY_LOOKUP_MAX_VALUE_BYTES as u64)
            .context("resolve authoritative StateService root node")?
            .is_some(),
        "authoritative pack does not contain the StateService root node"
    );
    println!("root node: reachable at 0x{}", hex::encode(root_key));
    Ok(())
}

fn read_checkpoint(pack_path: &Path) -> Result<CheckpointMarker> {
    let path = pack_path.join("checkpoint.json");
    let bytes =
        fs::read(&path).with_context(|| format!("read checkpoint marker {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("decode checkpoint marker {}", path.display()))
}

fn validate_checkpoint(
    checkpoint: &CheckpointMarker,
    network_magic: u32,
) -> Result<CheckpointBinding> {
    ensure!(
        checkpoint.schema_version == CHECKPOINT_SCHEMA_VERSION,
        "checkpoint schema {} is unsupported",
        checkpoint.schema_version
    );
    ensure!(
        checkpoint.complete && checkpoint.authoritative_ready,
        "checkpoint is not complete and explicitly authoritative-ready"
    );
    ensure!(
        checkpoint.source_backend == "mdbx"
            && checkpoint.source_namespace == STATE_SERVICE_NAMESPACE,
        "checkpoint source is not the MDBX StateService namespace"
    );
    ensure!(
        parse_checkpoint_network_magic(&checkpoint.network_magic)? == network_magic,
        "checkpoint network differs from --network-magic"
    );
    ensure!(
        (
            checkpoint.pack_segment_format_version,
            checkpoint.pack_frame_format_version,
            checkpoint.pack_index_format_version,
            checkpoint.pack_manifest_format_version,
        ) == (
            PACK_SEGMENT_FORMAT_VERSION,
            PACK_FRAME_FORMAT_VERSION,
            PACK_INDEX_FORMAT_VERSION,
            PACK_MANIFEST_FORMAT_VERSION,
        ),
        "checkpoint pack format tuple differs from this binary"
    );
    ensure!(
        checkpoint.rows > 0
            && checkpoint.frames > 0
            && checkpoint.tip_frame_end > PACK_SEGMENT_HEADER_LEN,
        "checkpoint pack geometry is empty"
    );
    ensure!(
        checkpoint.tip_segment_id <= checkpoint.tip_epoch,
        "checkpoint tip segment cannot contain a frame after the tip epoch"
    );
    ensure!(
        checkpoint.tip_epoch.checked_add(1) == Some(checkpoint.frames),
        "checkpoint tip epoch differs from its frame count"
    );
    ensure!(
        checkpoint.scrubbed_frames == checkpoint.frames
            && checkpoint.scrubbed_rows == checkpoint.rows
            && checkpoint.scrubbed_puts == checkpoint.rows
            && checkpoint.scrubbed_tombstones == 0
            && checkpoint.scrubbed_value_bytes == checkpoint.value_bytes,
        "checkpoint scrub evidence differs from its source geometry"
    );
    Ok(CheckpointBinding {
        source_root: decode_hash(
            &checkpoint.source_root_internal_bytes,
            "checkpoint source root",
        )?,
        store_identity: decode_hash(
            &checkpoint.source_namespace_sha256,
            "checkpoint source namespace digest",
        )?,
        tip_frame_sha256: decode_hash(&checkpoint.tip_frame_sha256, "checkpoint tip digest")?,
    })
}

fn read_state_tip(store: &RuntimeStore) -> Result<(u32, [u8; 32])> {
    let root = read_current_local_root(store).context("read current local StateService root")?;
    Ok((root.index(), root.root_hash().to_array()))
}

fn parse_u32(value: &str) -> Result<u32> {
    match value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        Some(hex) => u32::from_str_radix(hex, 16).context("hex value does not fit u32"),
        None => value
            .parse::<u32>()
            .context("decimal value does not fit u32"),
    }
}

include!("neo_pack_verify/tests.rs");
