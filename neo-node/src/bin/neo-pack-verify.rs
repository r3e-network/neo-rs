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
//!   neo-pack-verify --mode authority --network-magic <u32-or-hex>
//!     --mdbx <canonical-store-dir> --pack <authoritative-packs-dir>
//!     [--samples N] [--walk-cap N | --full-scan]
//!     [--scrub] [--scrub-indexes] [--maintain]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use neo_crypto::Sha256Hasher;
use neo_state_packs::authority::{AUTHORITATIVE_HIGH_WATER_KEY, AuthoritativeHighWaterRecord};
use neo_state_packs::shadow::{SHADOW_HIGH_WATER_KEY, ShadowHighWaterRecord};
use neo_state_packs::{
    CHECKPOINT_NAMESPACE_DIGEST_DOMAIN, PACK_FRAME_FORMAT_VERSION, PACK_INDEX_FORMAT_VERSION,
    PACK_KEY_BYTES, PACK_MANIFEST_FORMAT_VERSION, PackCommitHorizon, PackStore,
};
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::storage::StorageConfig;
use neo_storage::persistence::{RawReadOnlyStore, StoreFactory, TransactionalStore};
use serde::Deserialize;

const STATE_NODE_PREFIX: u8 = 0xf0;
const STATE_SERVICE_NAMESPACE: &str = "neo_state_service";
const CURRENT_LOCAL_ROOT_INDEX: &[u8] = &[0x02];
const STATE_ROOT_PREFIX: u8 = 0x01;
const STATE_ROOT_VALUE_ROOT_OFFSET: usize = 5;
const STATE_ROOT_VALUE_UNSIGNED_LEN: usize = 1 + 4 + 32;
const CHECKPOINT_SCHEMA_VERSION: u32 = 2;
const BATCH: usize = 8192;
const DEFAULT_MAX_INDEX_MEMORY_MB: u64 = 256;
const DEFAULT_WALK_CAP: u64 = 100_000;

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
    pack_frame_format_version: u32,
    pack_index_format_version: u32,
    pack_manifest_format_version: u32,
    tip_epoch: u64,
    tip_frame_end: u64,
    tip_payload_sha256: String,
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
    tip_payload_sha256: [u8; 32],
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
            samples,
            walk_cap,
            full_scan,
            maintain,
            scrub,
            scrub_indexes,
        )
        .map(|_| ());
    }

    // High-water marker first: it defines the shadow commit horizon.
    let high_water = match state_store.maintenance_metadata(SHADOW_HIGH_WATER_KEY)? {
        Some(record) => {
            let marker = ShadowHighWaterRecord::decode(&record)
                .context("high-water marker failed to decode")?;
            println!(
                "high-water: epoch={} frames={} node_ops={} value_bytes={} blocks={:?}..={:?} root=0x{}",
                marker.epoch,
                marker.frames_total,
                marker.node_operations,
                marker.node_put_value_bytes,
                marker.block_index_min,
                marker.block_index_max,
                marker
                    .state_root
                    .map(hex::encode)
                    .unwrap_or_else(|| "none".to_string()),
            );
            marker
        }
        None => bail!("high-water marker is absent; no shadow pack prefix is canonical"),
    };

    let max_index_memory_bytes = max_index_memory_mb
        .checked_mul(1024 * 1024)
        .context("--max-index-memory-mb overflows bytes")?;
    let mut pack = PackStore::open_at_commit_horizon(
        &pack_path,
        max_index_memory_bytes,
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
    samples: usize,
    walk_cap: u64,
    full_scan: bool,
    maintain: bool,
    scrub: bool,
    scrub_indexes: bool,
) -> Result<AuthorityState> {
    let checkpoint = read_checkpoint(pack_path)?;
    let binding = validate_checkpoint(&checkpoint, network_magic)?;
    let state_tip = read_state_tip(state_store)?;
    let durable_marker = state_store
        .maintenance_metadata(AUTHORITATIVE_HIGH_WATER_KEY)
        .context("read authoritative pack high-water marker")?;

    let (authority_state, horizon, expected_frame_end, expected_payload) = match durable_marker {
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
                "authority marker: epoch={} frame_end={} block={} root=0x{} payload_sha256=0x{}",
                marker.epoch,
                marker.frame_end,
                marker.block_index,
                hex::encode(marker.state_root),
                hex::encode(marker.frame_payload_sha256),
            );
            (
                AuthorityState::Marker,
                marker.commit_horizon(),
                marker.frame_end,
                marker.frame_payload_sha256,
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
                    payload_sha256: binding.tip_payload_sha256,
                },
                checkpoint.tip_frame_end,
                binding.tip_payload_sha256,
            )
        }
    };

    let max_index_memory_bytes = max_index_memory_mb
        .checked_mul(1024 * 1024)
        .context("--max-index-memory-mb overflows bytes")?;
    let mut pack =
        PackStore::open_at_commit_horizon(pack_path, max_index_memory_bytes, Some(horizon))
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
            && receipt.frame_end == expected_frame_end
            && receipt.payload_sha256 == expected_payload,
        "authoritative pack tip differs from its canonical checkpoint/marker"
    );
    if authority_state == AuthorityState::Checkpoint {
        ensure!(
            opened.frames == checkpoint.frames && opened.index_entries == checkpoint.rows,
            "checkpoint pack geometry differs from checkpoint.json"
        );
    }
    println!(
        "opened: frames={} runs={} index_entries={} tip_epoch={} tip_frame_end={}",
        opened.frames, opened.runs, opened.index_entries, receipt.epoch, receipt.frame_end,
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
        pack.maintain()
            .context("maintaining derived authoritative pack indexes")?;
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
    if scrub_indexes {
        let stats = pack
            .scrub_index_runs()
            .context("scrub every live authoritative index run")?;
        println!(
            "scrubbed indexes: runs={} v3_runs={} v4_runs={} records={} record_bytes={}",
            stats.runs, stats.v3_runs, stats.v4_runs, stats.records, stats.record_bytes,
        );
    }

    let mut root_key = [0u8; PACK_KEY_BYTES];
    root_key[0] = STATE_NODE_PREFIX;
    root_key[1..].copy_from_slice(&state_tip.1);
    ensure!(
        pack.get(&root_key)
            .context("resolve authoritative StateService root node")?
            .is_some(),
        "authoritative pack does not contain the StateService root node"
    );
    println!("root node: reachable at 0x{}", hex::encode(root_key));

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
        ensure!(
            read_state_tip(state_store)? == state_tip,
            "StateService tip changed during checkpoint verification"
        );
    } else {
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

fn compare_checkpoint_nodes(
    state_store: &RuntimeStore,
    pack: &PackStore,
    checkpoint: &CheckpointMarker,
    expected_digest: [u8; 32],
    samples: usize,
    walk_cap: u64,
    full_scan: bool,
) -> Result<()> {
    let mut hasher = Sha256Hasher::new();
    hasher.update(CHECKPOINT_NAMESPACE_DIGEST_DOMAIN);
    let mut rng = XorShift64(0x9E37_79B9_7F4A_7C15);
    let mut reservoir: Vec<([u8; PACK_KEY_BYTES], Vec<u8>)> = Vec::with_capacity(samples);
    let mut total_keys = 0u64;
    let mut total_value_bytes = 0u64;
    let maximum = (!full_scan).then_some(walk_cap);
    state_store.visit_raw_entries_with_prefix(&[STATE_NODE_PREFIX], maximum, |key, value| {
        if key.len() != PACK_KEY_BYTES || key.first() != Some(&STATE_NODE_PREFIX) {
            return Err(neo_storage::StorageError::invalid_operation(
                "StateService node scan returned a malformed key",
            ));
        }
        let key: [u8; PACK_KEY_BYTES] = key.try_into().expect("validated pack key");
        hasher.update(&(key.len() as u32).to_le_bytes());
        hasher.update(&key);
        hasher.update(&(value.len() as u64).to_le_bytes());
        hasher.update(value);
        total_value_bytes = total_value_bytes.saturating_add(value.len() as u64);
        if (total_keys as usize) < samples {
            reservoir.push((key, value.to_vec()));
        } else if samples != 0 {
            let index = rng.next() % (total_keys + 1);
            if (index as usize) < samples {
                reservoir[index as usize] = (key, value.to_vec());
            }
        }
        total_keys = total_keys.saturating_add(1);
        Ok(())
    })?;
    if full_scan {
        ensure!(
            total_keys == checkpoint.rows && total_value_bytes == checkpoint.value_bytes,
            "full MDBX node geometry differs from checkpoint.json"
        );
        ensure!(
            hasher.finalize() == expected_digest,
            "full MDBX node namespace digest differs from checkpoint.json"
        );
        println!(
            "full MDBX evidence: rows={} value_bytes={} digest=0x{}",
            total_keys,
            total_value_bytes,
            hex::encode(expected_digest),
        );
    } else if total_keys >= walk_cap {
        println!("walk capped at {walk_cap} keys (prefix-bounded sample)");
    }
    println!(
        "mdbx checkpoint node keys: {total_keys}; sampled: {}",
        reservoir.len()
    );

    reservoir.sort_unstable_by_key(|entry| entry.0);
    let mut matched = 0u64;
    let mut first_mismatch = None;
    for chunk in reservoir.chunks(BATCH) {
        let keys = chunk.iter().map(|(key, _)| *key).collect::<Vec<_>>();
        let values = pack.get_many_sorted(&keys)?;
        for ((key, expected), actual) in chunk.iter().zip(values) {
            if actual.as_deref() == Some(expected.as_slice()) {
                matched = matched.saturating_add(1);
            } else if first_mismatch.is_none() {
                first_mismatch = Some((*key, expected.clone(), actual.unwrap_or_default()));
            }
        }
    }
    println!("checkpoint sample matched: {matched}");
    if let Some((key, expected, actual)) = first_mismatch {
        bail!(
            "checkpoint differs from MDBX at 0x{}: mdbx {} bytes (0x{}...), pack {} bytes (0x{}...)",
            hex::encode(key),
            expected.len(),
            hex::encode(&expected[..expected.len().min(16)]),
            actual.len(),
            hex::encode(&actual[..actual.len().min(16)]),
        );
    }
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
            checkpoint.pack_frame_format_version,
            checkpoint.pack_index_format_version,
            checkpoint.pack_manifest_format_version,
        ) == (
            PACK_FRAME_FORMAT_VERSION,
            PACK_INDEX_FORMAT_VERSION,
            PACK_MANIFEST_FORMAT_VERSION,
        ),
        "checkpoint pack format tuple differs from this binary"
    );
    ensure!(
        checkpoint.rows > 0 && checkpoint.frames > 0 && checkpoint.tip_frame_end > 0,
        "checkpoint pack geometry is empty"
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
        tip_payload_sha256: decode_hash(&checkpoint.tip_payload_sha256, "checkpoint tip checksum")?,
    })
}

fn read_state_tip(store: &RuntimeStore) -> Result<(u32, [u8; 32])> {
    let index_bytes = store
        .try_get_bytes_result(CURRENT_LOCAL_ROOT_INDEX)
        .context("read StateService current local root index")?
        .context("StateService current local root index is absent")?;
    let index = u32::from_le_bytes(
        index_bytes
            .as_slice()
            .try_into()
            .context("StateService current local root index is not four bytes")?,
    );
    let mut key = Vec::with_capacity(5);
    key.push(STATE_ROOT_PREFIX);
    key.extend_from_slice(&index.to_be_bytes());
    let value = store
        .try_get_bytes_result(&key)
        .context("read StateService current root record")?
        .context("StateService current root record is absent")?;
    ensure!(
        value.len() >= STATE_ROOT_VALUE_UNSIGNED_LEN,
        "StateService current root record is truncated"
    );
    ensure!(
        value[0] == 0,
        "StateService current root record has an unsupported version"
    );
    ensure!(
        u32::from_le_bytes(value[1..5].try_into().expect("four-byte root index")) == index,
        "StateService root record index does not match its key"
    );
    Ok((
        index,
        value[STATE_ROOT_VALUE_ROOT_OFFSET..STATE_ROOT_VALUE_UNSIGNED_LEN]
            .try_into()
            .expect("fixed StateService root range"),
    ))
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

include!("../neo_pack_verify_tests.rs");

fn parse_checkpoint_network_magic(value: &str) -> Result<u32> {
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .context("checkpoint network magic lacks 0x prefix")?;
    u32::from_str_radix(value, 16).context("decode checkpoint network magic")
}

fn decode_hash(value: &str, field: &'static str) -> Result<[u8; 32]> {
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .with_context(|| format!("{field} lacks 0x prefix"))?;
    let bytes = hex::decode(value).with_context(|| format!("decode {field}"))?;
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| anyhow::anyhow!("{field} has {} bytes, expected 32", bytes.len()))
}
