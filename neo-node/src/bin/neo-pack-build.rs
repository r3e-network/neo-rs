//! Build a bounded offline node-pack checkpoint from the authoritative MDBX
//! StateService `0xf0 || node_hash` namespace.
//!
//! The source is streamed through one frozen MDBX cursor. A checkpoint marker
//! is published only after the source height/root remain stable and the pack
//! reopens successfully. Interrupted or bounded smoke builds therefore never
//! look like complete authoritative checkpoints.
//!
//! Usage:
//!   neo-pack-build --network-magic <u32-or-hex>
//!     --mdbx <canonical-store-dir> --pack <new-pack-dir>
//!     [--rows-per-frame N] [--max-rows N] [--max-index-memory-mb N]
//!     [--adopt-legacy-complete-pack | --adopt-legacy-partial-pack]

use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};
use neo_crypto::Sha256Hasher;
use neo_state_packs::{
    CHECKPOINT_NAMESPACE_DIGEST_DOMAIN, PACK_FRAME_FORMAT_VERSION, PACK_INDEX_FORMAT_VERSION,
    PACK_MANIFEST_FORMAT_VERSION, PACK_SEGMENT_FORMAT_VERSION, PackFrameReceipt, PackOpKind,
    PackOperation, PackSegmentId, PackStore, PackStoreConfig,
};
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::storage::StorageConfig;
use neo_storage::persistence::{RawReadOnlyStore, StoreFactory};
use neo_storage::{StorageError, StorageResult};
use serde::{Deserialize, Serialize};

const STATE_NODE_PREFIX: u8 = 0xf0;
const STATE_SERVICE_NAMESPACE: &str = "neo_state_service";
const CURRENT_LOCAL_ROOT_INDEX: &[u8] = &[0x02];
const DEFAULT_ROWS_PER_FRAME: usize = 1_000_000;
const DEFAULT_MAX_INDEX_MEMORY_MB: u64 = 512;
const STATE_ROOT_UNSIGNED_LEN: usize = 1 + 4 + 32;
const CHECKPOINT_SCHEMA_VERSION: u32 = 3;
const LEGACY_CHECKPOINT_SCHEMA_VERSION: u32 = 1;
const BUILD_IDENTITY_SCHEMA_VERSION: u32 = 2;
const BUILD_IDENTITY_FILE: &str = "checkpoint-build.json";
const BUILD_IDENTITY_TMP_FILE: &str = "checkpoint-build.json.tmp";

fn pack_store_config(max_index_memory_bytes: u64) -> Result<PackStoreConfig> {
    PackStoreConfig::default()
        .with_max_index_memory_bytes(max_index_memory_bytes)
        .context("validate checkpoint pack-store configuration")
}

#[derive(Clone, Debug)]
struct Arguments {
    network_magic: u32,
    mdbx_path: PathBuf,
    pack_path: PathBuf,
    rows_per_frame: usize,
    max_rows: Option<u64>,
    max_index_memory_bytes: u64,
    adopt_legacy_complete_pack: bool,
    adopt_legacy_partial_pack: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceTip {
    height: u32,
    root_internal: [u8; 32],
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct BuildIdentity {
    schema_version: u32,
    source_backend: String,
    source_namespace: String,
    network_magic: String,
    source_height: u32,
    source_root_internal_bytes: String,
    rows_per_frame: usize,
    pack_segment_format_version: u32,
    pack_frame_format_version: u32,
    pack_index_format_version: u32,
    pack_manifest_format_version: u32,
}

impl BuildIdentity {
    fn current(arguments: &Arguments, source_tip: SourceTip) -> Self {
        Self {
            schema_version: BUILD_IDENTITY_SCHEMA_VERSION,
            source_backend: "mdbx".to_owned(),
            source_namespace: STATE_SERVICE_NAMESPACE.to_owned(),
            network_magic: formatted_network_magic(arguments.network_magic),
            source_height: source_tip.height,
            source_root_internal_bytes: format!("0x{}", hex::encode(source_tip.root_internal)),
            rows_per_frame: arguments.rows_per_frame,
            pack_segment_format_version: PACK_SEGMENT_FORMAT_VERSION,
            pack_frame_format_version: PACK_FRAME_FORMAT_VERSION,
            pack_index_format_version: PACK_INDEX_FORMAT_VERSION,
            pack_manifest_format_version: PACK_MANIFEST_FORMAT_VERSION,
        }
    }
}

#[derive(Debug, Serialize)]
struct CheckpointReport {
    schema_version: u32,
    authoritative_ready: bool,
    complete: bool,
    source_backend: &'static str,
    source_namespace: &'static str,
    network_magic: String,
    source_height: u32,
    source_root: String,
    source_root_internal_bytes: String,
    source_namespace_sha256: String,
    rows: u64,
    resumed_rows: u64,
    value_bytes: u64,
    frames: u64,
    rows_per_frame: usize,
    pack_bytes: u64,
    live_index_bytes: u64,
    live_runs: u64,
    decoded_index_memory_bytes: u64,
    gc_runs_deleted: u64,
    gc_manifests_deleted: u64,
    gc_bytes_reclaimed: u64,
    pack_segment_format_version: u32,
    pack_frame_format_version: u32,
    pack_index_format_version: u32,
    pack_manifest_format_version: u32,
    tip_epoch: u64,
    tip_segment_id: u64,
    tip_frame_end: u64,
    tip_payload_sha256: String,
    scrubbed_frames: u64,
    scrubbed_rows: u64,
    scrubbed_puts: u64,
    scrubbed_tombstones: u64,
    scrubbed_payload_bytes: u64,
    scrubbed_value_bytes: u64,
    scrub_elapsed_seconds: f64,
    elapsed_seconds: f64,
}

#[derive(Debug, Deserialize)]
struct LegacyCheckpointReport {
    schema_version: u32,
    authoritative_ready: bool,
    complete: bool,
    source_backend: String,
    #[serde(default)]
    source_namespace: Option<String>,
    source_height: u32,
    source_root: String,
    source_root_internal_bytes: String,
    source_namespace_sha256: String,
    rows: u64,
    value_bytes: u64,
    frames: u64,
    rows_per_frame: usize,
    pack_bytes: u64,
    #[serde(default)]
    live_index_bytes: Option<u64>,
    #[serde(default)]
    live_runs: Option<u64>,
    #[serde(default)]
    decoded_index_memory_bytes: Option<u64>,
    #[serde(default)]
    tip_epoch: Option<u64>,
    #[serde(default)]
    tip_frame_end: Option<u64>,
    #[serde(default)]
    tip_payload_sha256: Option<String>,
}

struct OpenedCheckpoint {
    pack: PackStore,
    resumed_rows: u64,
    frames: u64,
    legacy: Option<LegacyCheckpointReport>,
    publish_identity_after_rows: Option<u64>,
}

fn main() -> Result<()> {
    let arguments = parse_arguments()?;
    let canonical: Arc<RuntimeStore> = StoreFactory::get_store_with_config(
        "mdbx",
        StorageConfig {
            path: arguments.mdbx_path.clone(),
            read_only: true,
            ..Default::default()
        },
    )
    .map_err(|error| anyhow::anyhow!("open MDBX store: {error}"))?;
    let state_store = canonical
        .open_coordinated_namespace(STATE_SERVICE_NAMESPACE)
        .context("open coordinated MDBX StateService namespace")?;
    let source_tip = read_source_tip(&state_store)?;

    let started = Instant::now();
    let OpenedCheckpoint {
        mut pack,
        resumed_rows,
        mut frames,
        legacy,
        mut publish_identity_after_rows,
    } = open_or_resume_pack(&arguments, source_tip)?;
    let mut operations = Vec::with_capacity(arguments.rows_per_frame);
    let mut hasher = Sha256Hasher::new();
    hasher.update(CHECKPOINT_NAMESPACE_DIGEST_DOMAIN);
    let mut rows = 0u64;
    let mut value_bytes = 0u64;
    let rows_per_frame = arguments.rows_per_frame;
    let visited = state_store.visit_raw_entries_with_prefix(
        &[STATE_NODE_PREFIX],
        arguments.max_rows,
        |key, value| {
            ensure_storage(
                key.len() == 33 && key.first() == Some(&STATE_NODE_PREFIX),
                "StateService node scan returned a malformed key",
            )?;
            let key: [u8; 33] = key.try_into().expect("validated 33-byte key");
            hasher.update(&(key.len() as u32).to_le_bytes());
            hasher.update(&key);
            hasher.update(&(value.len() as u64).to_le_bytes());
            hasher.update(value);
            rows = rows.saturating_add(1);
            value_bytes = value_bytes.saturating_add(value.len() as u64);
            operations.push(PackOperation {
                key,
                kind: PackOpKind::Put(value.to_vec()),
            });
            if operations.len() == rows_per_frame {
                if rows <= resumed_rows {
                    validate_existing_rows(&pack, &mut operations)?;
                    if publish_identity_after_rows == Some(rows) {
                        publish_adopted_partial_identity(&mut pack, &arguments, source_tip)?;
                        publish_identity_after_rows = None;
                    }
                } else {
                    append_frame(&mut pack, &mut operations)?;
                    frames = frames.saturating_add(1);
                }
                eprintln!(
                    "checkpoint progress: rows={rows} frames={frames} value_bytes={value_bytes}"
                );
            }
            Ok(())
        },
    )?;
    ensure!(visited == rows, "streamed row count changed unexpectedly");
    ensure!(
        publish_identity_after_rows.is_none(),
        "legacy partial checkpoint contains more rows than the frozen source"
    );
    ensure!(
        rows >= resumed_rows,
        "partial checkpoint contains more rows than the source namespace"
    );
    if !operations.is_empty() {
        if rows <= resumed_rows {
            validate_existing_rows(&pack, &mut operations)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        } else {
            append_frame(&mut pack, &mut operations)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            frames = frames.saturating_add(1);
        }
    }
    ensure!(rows > 0, "StateService node namespace is empty");

    let source_tip_after = read_source_tip(&state_store)?;
    ensure!(
        source_tip_after == source_tip,
        "source StateService height/root changed during checkpoint build"
    );
    let namespace_digest = hasher.finalize();
    if let Some(legacy) = &legacy {
        validate_legacy_scan_evidence(legacy, rows, value_bytes, namespace_digest)?;
    }
    let gc = pack.gc().context("reclaim derived checkpoint files")?;
    let scrub_started = Instant::now();
    let checkpoint_evidence = pack
        .scrub_checkpoint_namespace()
        .context("scrub and hash the complete committed checkpoint payload")?;
    let scrub = checkpoint_evidence.scrub;
    let scrub_elapsed_seconds = scrub_started.elapsed().as_secs_f64();
    ensure!(
        scrub.frames == frames
            && scrub.rows == rows
            && scrub.puts == rows
            && scrub.tombstones == 0
            && scrub.value_bytes == value_bytes,
        "checkpoint payload scrub does not match the frozen source geometry"
    );
    ensure!(
        checkpoint_evidence.sha256 == namespace_digest,
        "checkpoint pack namespace digest does not match the frozen source"
    );
    let (pack_bytes, live_index_bytes, live_runs, decoded_index_memory_bytes) = pack
        .layout()
        .context("inspect completed checkpoint layout")?;
    let tip = pack
        .last_frame_receipt()
        .context("completed checkpoint has no tip frame")?;
    ensure!(
        tip.epoch.checked_add(1) == Some(frames),
        "checkpoint tip epoch does not match its frame count"
    );
    verify_tip_payload_checksum(&arguments.pack_path, tip)?;
    if let Some(legacy) = &legacy {
        validate_legacy_pack_evidence(
            legacy,
            pack_bytes,
            live_index_bytes,
            live_runs,
            decoded_index_memory_bytes,
            tip,
        )?;
        publish_or_validate_build_identity(&arguments, source_tip)?;
    }
    let complete = arguments.max_rows.is_none();
    let report = CheckpointReport {
        schema_version: CHECKPOINT_SCHEMA_VERSION,
        // The report is published only by `publish_after_reopen`, after the
        // complete source scan, stable height/root check, payload scrub, tip
        // checksum verification, and pack reopen all succeed. Bounded builds
        // remain explicitly ineligible.
        authoritative_ready: complete,
        complete,
        source_backend: "mdbx",
        source_namespace: STATE_SERVICE_NAMESPACE,
        network_magic: formatted_network_magic(arguments.network_magic),
        source_height: source_tip.height,
        source_root: displayed_root(source_tip.root_internal),
        source_root_internal_bytes: format!("0x{}", hex::encode(source_tip.root_internal)),
        source_namespace_sha256: format!("0x{}", hex::encode(namespace_digest)),
        rows,
        resumed_rows,
        value_bytes,
        frames,
        rows_per_frame: arguments.rows_per_frame,
        pack_bytes,
        live_index_bytes,
        live_runs,
        decoded_index_memory_bytes,
        gc_runs_deleted: gc.runs_deleted,
        gc_manifests_deleted: gc.manifests_deleted,
        gc_bytes_reclaimed: gc.bytes_reclaimed,
        pack_segment_format_version: PACK_SEGMENT_FORMAT_VERSION,
        pack_frame_format_version: PACK_FRAME_FORMAT_VERSION,
        pack_index_format_version: PACK_INDEX_FORMAT_VERSION,
        pack_manifest_format_version: PACK_MANIFEST_FORMAT_VERSION,
        tip_epoch: tip.epoch,
        tip_segment_id: tip.segment_id.get(),
        tip_frame_end: tip.frame_end,
        tip_payload_sha256: format!("0x{}", hex::encode(tip.payload_sha256)),
        scrubbed_frames: scrub.frames,
        scrubbed_rows: scrub.rows,
        scrubbed_puts: scrub.puts,
        scrubbed_tombstones: scrub.tombstones,
        scrubbed_payload_bytes: scrub.payload_bytes,
        scrubbed_value_bytes: scrub.value_bytes,
        scrub_elapsed_seconds,
        elapsed_seconds: started.elapsed().as_secs_f64(),
    };
    publish_after_reopen(&arguments, &report, pack, legacy.is_some())?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    if !complete {
        eprintln!(
            "bounded smoke checkpoint is explicitly incomplete and cannot authorize pack mode"
        );
    }
    Ok(())
}

fn parse_arguments() -> Result<Arguments> {
    parse_arguments_from(std::env::args().skip(1))
}

fn parse_arguments_from(arguments: impl IntoIterator<Item = String>) -> Result<Arguments> {
    let mut network_magic = None;
    let mut mdbx_path = None;
    let mut pack_path = None;
    let mut rows_per_frame = DEFAULT_ROWS_PER_FRAME;
    let mut max_rows = None;
    let mut max_index_memory_mb = DEFAULT_MAX_INDEX_MEMORY_MB;
    let mut adopt_legacy_complete_pack = false;
    let mut adopt_legacy_partial_pack = false;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--network-magic" => {
                let value = arguments
                    .next()
                    .context("--network-magic requires a u32 or 0x-prefixed hex value")?;
                network_magic = Some(
                    parse_u32_literal(&value)
                        .with_context(|| format!("invalid --network-magic value {value}"))?,
                );
            }
            "--mdbx" => mdbx_path = arguments.next().map(PathBuf::from),
            "--pack" => pack_path = arguments.next().map(PathBuf::from),
            "--rows-per-frame" => {
                rows_per_frame = parse_positive(&mut arguments, "--rows-per-frame")?;
            }
            "--max-rows" => {
                max_rows = Some(parse_positive::<u64>(&mut arguments, "--max-rows")?);
            }
            "--max-index-memory-mb" => {
                max_index_memory_mb = parse_positive(&mut arguments, "--max-index-memory-mb")?;
            }
            "--adopt-legacy-complete-pack" => adopt_legacy_complete_pack = true,
            "--adopt-legacy-partial-pack" => adopt_legacy_partial_pack = true,
            other => bail!("unknown argument {other}"),
        }
    }
    ensure!(
        !(adopt_legacy_complete_pack && adopt_legacy_partial_pack),
        "legacy complete-pack and partial-pack adoption are mutually exclusive"
    );
    ensure!(
        !(adopt_legacy_complete_pack || adopt_legacy_partial_pack) || max_rows.is_none(),
        "legacy pack adoption cannot be combined with --max-rows"
    );
    let max_index_memory_bytes = max_index_memory_mb
        .checked_mul(1024 * 1024)
        .context("--max-index-memory-mb overflows bytes")?;
    Ok(Arguments {
        network_magic: network_magic.context("--network-magic is required")?,
        mdbx_path: mdbx_path.context("--mdbx is required")?,
        pack_path: pack_path.context("--pack is required")?,
        rows_per_frame,
        max_rows,
        max_index_memory_bytes,
        adopt_legacy_complete_pack,
        adopt_legacy_partial_pack,
    })
}

fn parse_u32_literal(value: &str) -> Result<u32> {
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

fn parse_positive<T>(arguments: &mut impl Iterator<Item = String>, name: &str) -> Result<T>
where
    T: std::str::FromStr + PartialEq + Default,
    T::Err: std::fmt::Display,
{
    let value = arguments
        .next()
        .with_context(|| format!("{name} requires a number"))?
        .parse::<T>()
        .map_err(|error| anyhow::anyhow!("{name} requires a positive number: {error}"))?;
    ensure!(value != T::default(), "{name} must be greater than zero");
    Ok(value)
}

fn read_source_tip(store: &RuntimeStore) -> Result<SourceTip> {
    let index = store
        .try_get_bytes_result(CURRENT_LOCAL_ROOT_INDEX)?
        .context("StateService current local root index is absent")?;
    let index: [u8; 4] = index
        .try_into()
        .map_err(|_| anyhow::anyhow!("StateService current local root index is malformed"))?;
    let height = u32::from_le_bytes(index);
    let mut root_key = Vec::with_capacity(5);
    root_key.push(0x01);
    root_key.extend_from_slice(&height.to_be_bytes());
    let root = store
        .try_get_bytes_result(&root_key)?
        .with_context(|| format!("StateService root record {height} is absent"))?;
    ensure!(
        root.len() >= STATE_ROOT_UNSIGNED_LEN,
        "StateService root record {height} is malformed"
    );
    ensure!(
        root[0] == 0,
        "StateService root record {height} has an unsupported version"
    );
    ensure!(
        u32::from_le_bytes(root[1..5].try_into().expect("four-byte index")) == height,
        "StateService root record index does not match the current pointer"
    );
    let root_internal = root[5..37].try_into().expect("32-byte root");
    Ok(SourceTip {
        height,
        root_internal,
    })
}

fn open_or_resume_pack(arguments: &Arguments, source_tip: SourceTip) -> Result<OpenedCheckpoint> {
    let checkpoint = arguments.pack_path.join("checkpoint.json");
    let checkpoint_tmp = arguments.pack_path.join("checkpoint.json.tmp");
    ensure!(
        !checkpoint_tmp.exists(),
        "checkpoint publication is incomplete at {}; inspect it before retrying",
        checkpoint_tmp.display()
    );
    let build_identity_tmp = arguments.pack_path.join(BUILD_IDENTITY_TMP_FILE);
    ensure!(
        !build_identity_tmp.exists(),
        "checkpoint build identity publication is incomplete at {}; inspect it before retrying",
        build_identity_tmp.display()
    );
    if arguments.adopt_legacy_complete_pack {
        return open_legacy_complete_pack(arguments, source_tip, &checkpoint);
    }
    if arguments.adopt_legacy_partial_pack {
        return open_legacy_partial_pack(arguments, &checkpoint);
    }
    ensure!(
        !checkpoint.exists(),
        "checkpoint marker already exists at {}; refusing to overwrite it",
        checkpoint.display()
    );
    let expected_identity = BuildIdentity::current(arguments, source_tip);
    if arguments
        .pack_path
        .join(PackSegmentId::INITIAL.file_name())
        .exists()
    {
        let actual_identity = read_build_identity(&arguments.pack_path)?;
        validate_build_identity(&actual_identity, &expected_identity)?;
        let pack = PackStore::open(
            &arguments.pack_path,
            pack_store_config(arguments.max_index_memory_bytes)?,
        )
        .with_context(|| {
            format!(
                "reopen partial checkpoint at {}",
                arguments.pack_path.display()
            )
        })?;
        let validation = pack.open_validation();
        ensure!(
            validation.frames > 0 && validation.index_entries > 0,
            "partial checkpoint has no committed rows"
        );
        let rows_per_frame =
            u64::try_from(arguments.rows_per_frame).context("--rows-per-frame does not fit u64")?;
        let expected_rows = validation
            .frames
            .checked_mul(rows_per_frame)
            .context("partial checkpoint frame geometry overflows u64")?;
        ensure!(
            validation.index_entries == expected_rows,
            "partial checkpoint has {} live rows across {} frames, not exactly {} rows per frame",
            validation.index_entries,
            validation.frames,
            arguments.rows_per_frame
        );
        eprintln!(
            "checkpoint resume: validating {} existing rows across {} frames",
            validation.index_entries, validation.frames
        );
        return Ok(OpenedCheckpoint {
            pack,
            resumed_rows: validation.index_entries,
            frames: validation.frames,
            legacy: None,
            publish_identity_after_rows: None,
        });
    }
    let build_identity = arguments.pack_path.join(BUILD_IDENTITY_FILE);
    ensure!(
        !build_identity.exists(),
        "checkpoint build identity exists without a pack at {}; refusing ambiguous recovery",
        build_identity.display()
    );
    let pack = PackStore::create(
        &arguments.pack_path,
        pack_store_config(arguments.max_index_memory_bytes)?,
    )
    .with_context(|| {
        format!(
            "create checkpoint pack at {}",
            arguments.pack_path.display()
        )
    })?;
    write_build_identity(&arguments.pack_path, &expected_identity)?;
    Ok(OpenedCheckpoint {
        pack,
        resumed_rows: 0,
        frames: 0,
        legacy: None,
        publish_identity_after_rows: None,
    })
}

fn open_legacy_complete_pack(
    arguments: &Arguments,
    source_tip: SourceTip,
    checkpoint: &Path,
) -> Result<OpenedCheckpoint> {
    ensure!(
        checkpoint.exists(),
        "--adopt-legacy-complete-pack requires an existing checkpoint.json"
    );
    let file = File::open(checkpoint)
        .with_context(|| format!("open legacy checkpoint marker {}", checkpoint.display()))?;
    let legacy: LegacyCheckpointReport = serde_json::from_reader(BufReader::new(file))
        .with_context(|| format!("decode legacy checkpoint marker {}", checkpoint.display()))?;
    validate_legacy_source_identity(&legacy, arguments, source_tip)?;

    let identity_path = arguments.pack_path.join(BUILD_IDENTITY_FILE);
    if identity_path.exists() {
        let actual = read_build_identity(&arguments.pack_path)?;
        let expected = BuildIdentity::current(arguments, source_tip);
        validate_build_identity(&actual, &expected)
            .context("validate an interrupted legacy adoption identity")?;
    }

    let pack = PackStore::open(
        &arguments.pack_path,
        pack_store_config(arguments.max_index_memory_bytes)?,
    )
    .with_context(|| {
        format!(
            "reopen legacy complete checkpoint at {}",
            arguments.pack_path.display()
        )
    })?;
    let validation = pack.open_validation();
    ensure!(
        validation.frames == legacy.frames,
        "legacy checkpoint frame count differs from the pack"
    );
    ensure!(
        validation.index_entries == legacy.rows,
        "legacy checkpoint row count differs from the pack"
    );
    let rows_per_frame =
        u64::try_from(legacy.rows_per_frame).context("legacy rows_per_frame does not fit u64")?;
    let expected_frames = legacy
        .rows
        .checked_sub(1)
        .context("legacy checkpoint has zero rows")?
        .checked_div(rows_per_frame)
        .and_then(|frames_before_tip| frames_before_tip.checked_add(1))
        .context("legacy checkpoint frame geometry overflows u64")?;
    ensure!(
        legacy.frames == expected_frames,
        "legacy checkpoint row/frame geometry is inconsistent"
    );
    let tip = pack
        .last_frame_receipt()
        .context("legacy complete checkpoint has no tip frame")?;
    let (pack_bytes, live_index_bytes, live_runs, decoded_index_memory_bytes) = pack
        .layout()
        .context("inspect legacy complete checkpoint layout")?;
    validate_legacy_pack_evidence(
        &legacy,
        pack_bytes,
        live_index_bytes,
        live_runs,
        decoded_index_memory_bytes,
        tip,
    )?;
    verify_tip_payload_checksum(&arguments.pack_path, tip)?;
    eprintln!(
        "legacy complete checkpoint adoption: validating {} exact source rows across {} frames",
        legacy.rows, legacy.frames
    );
    Ok(OpenedCheckpoint {
        pack,
        resumed_rows: legacy.rows,
        frames: legacy.frames,
        legacy: Some(legacy),
        publish_identity_after_rows: None,
    })
}

fn open_legacy_partial_pack(arguments: &Arguments, checkpoint: &Path) -> Result<OpenedCheckpoint> {
    ensure!(
        !checkpoint.exists(),
        "--adopt-legacy-partial-pack requires checkpoint.json to be absent"
    );
    let identity_path = arguments.pack_path.join(BUILD_IDENTITY_FILE);
    ensure!(
        !identity_path.exists(),
        "legacy partial adoption found checkpoint-build.json; resume without the adoption flag"
    );
    ensure!(
        arguments
            .pack_path
            .join(PackSegmentId::INITIAL.file_name())
            .exists(),
        "--adopt-legacy-partial-pack requires an existing initial pack segment"
    );
    let pack = PackStore::open(
        &arguments.pack_path,
        pack_store_config(arguments.max_index_memory_bytes)?,
    )
    .with_context(|| {
        format!(
            "reopen legacy partial checkpoint at {}",
            arguments.pack_path.display()
        )
    })?;
    let validation = pack.open_validation();
    ensure!(
        validation.frames > 0 && validation.index_entries > 0,
        "legacy partial checkpoint has no committed rows"
    );
    let rows_per_frame =
        u64::try_from(arguments.rows_per_frame).context("--rows-per-frame does not fit u64")?;
    let expected_rows = validation
        .frames
        .checked_mul(rows_per_frame)
        .context("legacy partial checkpoint frame geometry overflows u64")?;
    ensure!(
        validation.index_entries == expected_rows,
        "legacy partial checkpoint has {} live rows across {} frames, not exactly {} rows per frame",
        validation.index_entries,
        validation.frames,
        arguments.rows_per_frame
    );
    eprintln!(
        "legacy partial checkpoint adoption: validating {} exact source rows across {} frames before publishing build identity",
        validation.index_entries, validation.frames
    );
    Ok(OpenedCheckpoint {
        pack,
        resumed_rows: validation.index_entries,
        frames: validation.frames,
        legacy: None,
        publish_identity_after_rows: Some(validation.index_entries),
    })
}

fn publish_adopted_partial_identity(
    pack: &mut PackStore,
    arguments: &Arguments,
    source_tip: SourceTip,
) -> StorageResult<()> {
    pack.republish_manifest()
        .map_err(|error| StorageError::Backend {
            message: format!("upgrade validated legacy partial manifest: {error:#}"),
        })?;
    write_build_identity(
        &arguments.pack_path,
        &BuildIdentity::current(arguments, source_tip),
    )
    .map_err(|error| StorageError::Backend {
        message: format!("publish validated legacy partial build identity: {error:#}"),
    })
}

fn read_build_identity(pack_path: &Path) -> Result<BuildIdentity> {
    let path = pack_path.join(BUILD_IDENTITY_FILE);
    let file = File::open(&path).with_context(|| {
        format!(
            "open checkpoint build identity {}; legacy partial packs must be rebuilt",
            path.display()
        )
    })?;
    serde_json::from_reader(BufReader::new(file))
        .with_context(|| format!("decode checkpoint build identity {}", path.display()))
}

fn validate_build_identity(actual: &BuildIdentity, expected: &BuildIdentity) -> Result<()> {
    ensure!(
        actual.schema_version == expected.schema_version,
        "checkpoint build identity schema version changed"
    );
    ensure!(
        actual.source_backend == expected.source_backend
            && actual.source_namespace == expected.source_namespace,
        "checkpoint build source backend or namespace changed"
    );
    ensure!(
        actual.network_magic == expected.network_magic,
        "checkpoint build network magic changed from {} to {}",
        actual.network_magic,
        expected.network_magic
    );
    ensure!(
        actual.source_height == expected.source_height
            && actual.source_root_internal_bytes == expected.source_root_internal_bytes,
        "checkpoint build source height or root changed"
    );
    ensure!(
        actual.rows_per_frame == expected.rows_per_frame,
        "checkpoint build rows per frame changed from {} to {}",
        actual.rows_per_frame,
        expected.rows_per_frame
    );
    ensure!(
        actual.pack_segment_format_version == expected.pack_segment_format_version
            && actual.pack_frame_format_version == expected.pack_frame_format_version
            && actual.pack_index_format_version == expected.pack_index_format_version
            && actual.pack_manifest_format_version == expected.pack_manifest_format_version,
        "checkpoint pack format version changed"
    );
    Ok(())
}

fn validate_legacy_source_identity(
    legacy: &LegacyCheckpointReport,
    arguments: &Arguments,
    source_tip: SourceTip,
) -> Result<()> {
    ensure!(
        legacy.schema_version == LEGACY_CHECKPOINT_SCHEMA_VERSION,
        "legacy adoption accepts only checkpoint schema version {}",
        LEGACY_CHECKPOINT_SCHEMA_VERSION
    );
    ensure!(
        legacy.complete,
        "legacy checkpoint is not a complete namespace snapshot"
    );
    ensure!(
        !legacy.authoritative_ready,
        "legacy checkpoint has an unexpected authoritative-ready claim"
    );
    ensure!(
        legacy.source_backend == "mdbx",
        "legacy checkpoint source backend is not MDBX"
    );
    if let Some(namespace) = &legacy.source_namespace {
        ensure!(
            namespace == STATE_SERVICE_NAMESPACE,
            "legacy checkpoint source namespace changed"
        );
    }
    ensure!(
        arguments.max_rows.is_none(),
        "legacy adoption requires an uncapped source scan"
    );
    ensure!(
        legacy.rows > 0 && legacy.frames > 0 && legacy.rows_per_frame > 0,
        "legacy checkpoint has zero rows, frames, or rows_per_frame"
    );
    ensure!(
        legacy.rows_per_frame == arguments.rows_per_frame,
        "legacy checkpoint rows_per_frame differs from --rows-per-frame"
    );
    ensure!(
        legacy.source_height == source_tip.height,
        "legacy checkpoint source height differs from the frozen source"
    );
    ensure!(
        legacy.source_root == displayed_root(source_tip.root_internal),
        "legacy checkpoint displayed root differs from the frozen source"
    );
    ensure!(
        parse_prefixed_hex_32(
            &legacy.source_root_internal_bytes,
            "legacy source root internal bytes"
        )? == source_tip.root_internal,
        "legacy checkpoint internal root differs from the frozen source"
    );
    parse_prefixed_hex_32(
        &legacy.source_namespace_sha256,
        "legacy source namespace digest",
    )?;
    Ok(())
}

fn validate_legacy_scan_evidence(
    legacy: &LegacyCheckpointReport,
    rows: u64,
    value_bytes: u64,
    namespace_digest: [u8; 32],
) -> Result<()> {
    ensure!(
        rows == legacy.rows,
        "legacy checkpoint row count differs from the frozen source scan"
    );
    ensure!(
        value_bytes == legacy.value_bytes,
        "legacy checkpoint value bytes differ from the frozen source scan"
    );
    ensure!(
        parse_prefixed_hex_32(
            &legacy.source_namespace_sha256,
            "legacy source namespace digest"
        )? == namespace_digest,
        "legacy checkpoint namespace digest differs from the frozen source scan"
    );
    Ok(())
}

fn validate_legacy_pack_evidence(
    legacy: &LegacyCheckpointReport,
    pack_bytes: u64,
    live_index_bytes: u64,
    live_runs: u64,
    decoded_index_memory_bytes: u64,
    tip: PackFrameReceipt,
) -> Result<()> {
    ensure!(
        pack_bytes == legacy.pack_bytes,
        "legacy checkpoint pack byte length differs from the pack"
    );
    if let Some(expected) = legacy.live_index_bytes {
        ensure!(
            live_index_bytes == expected,
            "legacy checkpoint live index bytes differ from the pack"
        );
    }
    if let Some(expected) = legacy.live_runs {
        ensure!(
            live_runs == expected,
            "legacy checkpoint live run count differs from the pack"
        );
    }
    if let Some(expected) = legacy.decoded_index_memory_bytes {
        ensure!(
            decoded_index_memory_bytes == expected,
            "legacy checkpoint decoded index memory differs from the pack"
        );
    }
    ensure!(
        tip.epoch.checked_add(1) == Some(legacy.frames),
        "legacy checkpoint tip epoch differs from the frame count"
    );
    ensure!(
        tip.frame_end == pack_bytes,
        "legacy checkpoint tip does not end at the pack boundary"
    );
    let rows_per_frame =
        u64::try_from(legacy.rows_per_frame).context("legacy rows_per_frame does not fit u64")?;
    let rows_before_tip = legacy
        .frames
        .checked_sub(1)
        .and_then(|frames| frames.checked_mul(rows_per_frame))
        .context("legacy checkpoint tip row geometry overflows u64")?;
    ensure!(
        legacy.rows.checked_sub(rows_before_tip) == Some(tip.rows),
        "legacy checkpoint tip row count differs from the declared geometry"
    );
    match (
        legacy.tip_epoch,
        legacy.tip_frame_end,
        legacy.tip_payload_sha256.as_deref(),
    ) {
        (None, None, None) => {}
        (Some(epoch), Some(frame_end), Some(payload_sha256)) => {
            ensure!(
                epoch == tip.epoch
                    && frame_end == tip.frame_end
                    && parse_prefixed_hex_32(payload_sha256, "legacy tip payload checksum")?
                        == tip.payload_sha256,
                "legacy checkpoint tip identity differs from the pack"
            );
        }
        _ => bail!("legacy checkpoint has an incomplete optional tip identity"),
    }
    Ok(())
}

fn parse_prefixed_hex_32(value: &str, label: &str) -> Result<[u8; 32]> {
    let encoded = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .with_context(|| format!("{label} must start with 0x"))?;
    let bytes = hex::decode(encoded).with_context(|| format!("{label} is not valid hex"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("{label} must contain exactly 32 bytes"))
}

fn publish_or_validate_build_identity(arguments: &Arguments, source_tip: SourceTip) -> Result<()> {
    let expected = BuildIdentity::current(arguments, source_tip);
    let path = arguments.pack_path.join(BUILD_IDENTITY_FILE);
    if path.exists() {
        let actual = read_build_identity(&arguments.pack_path)?;
        validate_build_identity(&actual, &expected)
            .context("validate previously published legacy adoption identity")
    } else {
        write_build_identity(&arguments.pack_path, &expected)
    }
}

fn verify_tip_payload_checksum(pack_path: &Path, tip: PackFrameReceipt) -> Result<()> {
    let path = pack_path.join(tip.segment_id.file_name());
    let mut file = File::open(&path)
        .with_context(|| format!("open checkpoint pack {} for tip scrub", path.display()))?;
    let file_bytes = file
        .metadata()
        .with_context(|| format!("stat checkpoint pack {}", path.display()))?
        .len();
    ensure!(
        file_bytes == tip.frame_end,
        "checkpoint tip does not match the physical pack length"
    );
    let payload_start = tip
        .frame_end
        .checked_sub(tip.payload_bytes)
        .context("checkpoint tip payload range underflows")?;
    ensure!(
        payload_start > tip.frame_start,
        "checkpoint tip payload does not follow its frame header"
    );
    file.seek(SeekFrom::Start(payload_start))
        .with_context(|| format!("seek checkpoint tip payload in {}", path.display()))?;
    let mut hasher = Sha256Hasher::new();
    let mut remaining = tip.payload_bytes;
    let mut buffer = vec![0u8; 1024 * 1024];
    while remaining > 0 {
        let chunk = usize::try_from(remaining.min(buffer.len() as u64))
            .expect("bounded checksum chunk fits usize");
        file.read_exact(&mut buffer[..chunk])
            .with_context(|| format!("read checkpoint tip payload in {}", path.display()))?;
        hasher.update(&buffer[..chunk]);
        remaining -= chunk as u64;
    }
    ensure!(
        hasher.finalize() == tip.payload_sha256,
        "checkpoint tip payload checksum differs from its frame receipt"
    );
    Ok(())
}

fn write_build_identity(pack_path: &Path, identity: &BuildIdentity) -> Result<()> {
    let temporary = pack_path.join(BUILD_IDENTITY_TMP_FILE);
    write_json_new(&temporary, identity, "checkpoint build identity")?;
    let final_path = pack_path.join(BUILD_IDENTITY_FILE);
    fs::rename(&temporary, &final_path).with_context(|| {
        format!(
            "publish checkpoint build identity {} as {}",
            temporary.display(),
            final_path.display()
        )
    })?;
    sync_directory(pack_path)
}

fn validate_existing_rows(
    pack: &PackStore,
    operations: &mut Vec<PackOperation>,
) -> StorageResult<()> {
    let keys = operations
        .iter()
        .map(|operation| operation.key)
        .collect::<Vec<_>>();
    let values = pack
        .get_many_sorted(&keys)
        .map_err(|error| StorageError::Backend {
            message: format!("partial checkpoint lookup failed: {error:#}"),
        })?;
    for (operation, actual) in operations.iter().zip(values) {
        let PackOpKind::Put(expected) = &operation.kind else {
            return Err(StorageError::invalid_operation(
                "partial checkpoint contains a tombstone in a base snapshot",
            ));
        };
        if actual.as_deref() != Some(expected.as_slice()) {
            return Err(StorageError::invalid_operation(format!(
                "partial checkpoint differs from the source at 0x{}",
                hex::encode(operation.key)
            )));
        }
    }
    operations.clear();
    Ok(())
}

fn append_frame(pack: &mut PackStore, operations: &mut Vec<PackOperation>) -> StorageResult<()> {
    pack.append(operations)
        .map_err(|error| StorageError::Backend {
            message: format!("checkpoint pack append failed: {error:#}"),
        })?;
    operations.clear();
    Ok(())
}

fn ensure_storage(condition: bool, message: &str) -> StorageResult<()> {
    if condition {
        Ok(())
    } else {
        Err(StorageError::invalid_operation(message))
    }
}

fn displayed_root(mut internal: [u8; 32]) -> String {
    internal.reverse();
    format!("0x{}", hex::encode(internal))
}

fn formatted_network_magic(network_magic: u32) -> String {
    format!("0x{network_magic:08X}")
}

fn publish_after_reopen(
    arguments: &Arguments,
    report: &CheckpointReport,
    pack: PackStore,
    replace_legacy_checkpoint: bool,
) -> Result<()> {
    let checkpoint_tmp = arguments.pack_path.join("checkpoint.json.tmp");
    write_checkpoint_temp(&checkpoint_tmp, report)?;
    drop(pack);

    let reopened = PackStore::open(
        &arguments.pack_path,
        pack_store_config(arguments.max_index_memory_bytes)?,
    )
    .context("reopen completed checkpoint pack")?;
    let validation = reopened.open_validation();
    ensure!(
        validation.frames == report.frames,
        "reopened checkpoint exposes a different frame count"
    );
    ensure!(
        validation.index_entries == report.rows,
        "reopened checkpoint exposes a different row count"
    );
    let reopened_tip = reopened
        .last_frame_receipt()
        .context("reopened checkpoint has no tip frame")?;
    verify_tip_payload_checksum(&arguments.pack_path, reopened_tip)?;
    ensure!(
        reopened_tip.epoch == report.tip_epoch
            && reopened_tip.segment_id.get() == report.tip_segment_id
            && reopened_tip.frame_end == report.tip_frame_end
            && format!("0x{}", hex::encode(reopened_tip.payload_sha256))
                == report.tip_payload_sha256,
        "reopened checkpoint exposes a different tip frame"
    );
    drop(reopened);

    let checkpoint = arguments.pack_path.join("checkpoint.json");
    ensure!(
        replace_legacy_checkpoint == checkpoint.exists(),
        "checkpoint replacement state changed before publication"
    );
    fs::rename(&checkpoint_tmp, &checkpoint).with_context(|| {
        format!(
            "publish checkpoint marker {} as {}",
            checkpoint_tmp.display(),
            checkpoint.display()
        )
    })?;
    sync_directory(&arguments.pack_path)
}

fn write_checkpoint_temp(path: &Path, report: &CheckpointReport) -> Result<()> {
    write_json_new(path, report, "checkpoint marker")
}

fn write_json_new<T: Serialize + ?Sized>(path: &Path, value: &T, kind: &str) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .with_context(|| format!("create {kind} {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("write {kind} {}", path.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("finish {kind} {}", path.display()))?;
    file.sync_data()
        .with_context(|| format!("sync {kind} {}", path.display()))
}

fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)
        .with_context(|| format!("open directory {} for sync", path.display()))?
        .sync_all()
        .with_context(|| format!("sync directory {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use tempfile::tempdir;

    struct LegacyFixture {
        arguments: Arguments,
        source_tip: SourceTip,
        operations: Vec<PackOperation>,
        marker: Value,
        tip: PackFrameReceipt,
    }

    fn create_legacy_fixture(pack_path: &Path) -> LegacyFixture {
        let arguments = Arguments {
            network_magic: 0x334F_454E,
            mdbx_path: PathBuf::new(),
            pack_path: pack_path.to_path_buf(),
            rows_per_frame: 2,
            max_rows: None,
            max_index_memory_bytes: 1024 * 1024,
            adopt_legacy_complete_pack: true,
            adopt_legacy_partial_pack: false,
        };
        let source_tip = SourceTip {
            height: 123,
            root_internal: [0x55; 32],
        };
        let operations = (1u8..=3)
            .map(|suffix| {
                let mut key = [suffix; 33];
                key[0] = STATE_NODE_PREFIX;
                PackOperation {
                    key,
                    kind: PackOpKind::Put(vec![suffix; usize::from(suffix) + 2]),
                }
            })
            .collect::<Vec<_>>();
        let mut pack = PackStore::create(
            pack_path,
            pack_store_config(arguments.max_index_memory_bytes)
                .expect("valid legacy pack configuration"),
        )
        .expect("create legacy pack");
        pack.append(&operations[..2])
            .expect("append full legacy frame");
        pack.append(&operations[2..])
            .expect("append partial legacy tip frame");
        let gc = pack.gc().expect("collect legacy derived files");
        let (pack_bytes, live_index_bytes, live_runs, decoded_index_memory_bytes) =
            pack.layout().expect("inspect legacy layout");
        let tip = pack.last_frame_receipt().expect("legacy tip receipt");
        drop(pack);

        let (namespace_digest, value_bytes) = namespace_evidence(&operations);
        let marker = json!({
            "schema_version": LEGACY_CHECKPOINT_SCHEMA_VERSION,
            "authoritative_ready": false,
            "complete": true,
            "source_backend": "mdbx",
            "source_namespace": STATE_SERVICE_NAMESPACE,
            "network_magic": "0xDEADBEEF",
            "source_height": source_tip.height,
            "source_root": displayed_root(source_tip.root_internal),
            "source_root_internal_bytes": format!("0x{}", hex::encode(source_tip.root_internal)),
            "source_namespace_sha256": format!("0x{}", hex::encode(namespace_digest)),
            "rows": operations.len() as u64,
            "resumed_rows": 0,
            "value_bytes": value_bytes,
            "frames": 2,
            "rows_per_frame": arguments.rows_per_frame,
            "pack_bytes": pack_bytes,
            "live_index_bytes": live_index_bytes,
            "live_runs": live_runs,
            "decoded_index_memory_bytes": decoded_index_memory_bytes,
            "gc_runs_deleted": gc.runs_deleted,
            "gc_manifests_deleted": gc.manifests_deleted,
            "gc_bytes_reclaimed": gc.bytes_reclaimed,
            "elapsed_seconds": 1.0
        });
        LegacyFixture {
            arguments,
            source_tip,
            operations,
            marker,
            tip,
        }
    }

    fn namespace_evidence(operations: &[PackOperation]) -> ([u8; 32], u64) {
        let mut hasher = Sha256Hasher::new();
        hasher.update(CHECKPOINT_NAMESPACE_DIGEST_DOMAIN);
        let mut value_bytes = 0u64;
        for operation in operations {
            let PackOpKind::Put(value) = &operation.kind else {
                panic!("legacy base fixture must contain only puts");
            };
            hasher.update(&(operation.key.len() as u32).to_le_bytes());
            hasher.update(&operation.key);
            hasher.update(&(value.len() as u64).to_le_bytes());
            hasher.update(value);
            value_bytes += value.len() as u64;
        }
        (hasher.finalize(), value_bytes)
    }

    fn write_legacy_marker(pack_path: &Path, marker: &Value) {
        fs::write(
            pack_path.join("checkpoint.json"),
            serde_json::to_vec_pretty(marker).expect("encode legacy checkpoint"),
        )
        .expect("write legacy checkpoint");
    }

    #[test]
    fn displayed_root_reverses_internal_uint256_bytes() {
        let mut internal = [0u8; 32];
        internal[0] = 0x11;
        internal[31] = 0xaa;
        let displayed = displayed_root(internal);
        assert!(displayed.starts_with("0xaa"));
        assert!(displayed.ends_with("11"));
    }

    #[test]
    fn legacy_complete_adoption_reissues_current_identity_from_cli_and_verified_pack() {
        let temporary = tempdir().expect("temporary checkpoint parent");
        let pack_path = temporary.path().join("pack");
        let fixture = create_legacy_fixture(&pack_path);
        write_legacy_marker(&pack_path, &fixture.marker);

        let opened = open_or_resume_pack(&fixture.arguments, fixture.source_tip)
            .expect("open complete legacy checkpoint");
        assert_eq!(opened.resumed_rows, fixture.operations.len() as u64);
        assert_eq!(opened.frames, 2);
        let legacy = opened.legacy.as_ref().expect("legacy adoption evidence");
        let mut exact_rows = fixture.operations.clone();
        validate_existing_rows(&opened.pack, &mut exact_rows)
            .expect("validate every legacy row exactly");
        let (namespace_digest, value_bytes) = namespace_evidence(&fixture.operations);
        validate_legacy_scan_evidence(
            legacy,
            fixture.operations.len() as u64,
            value_bytes,
            namespace_digest,
        )
        .expect("validate legacy namespace evidence");

        let (pack_bytes, live_index_bytes, live_runs, decoded_index_memory_bytes) =
            opened.pack.layout().expect("inspect adopted pack");
        let tip = opened.pack.last_frame_receipt().expect("adopted tip");
        let scrub = opened
            .pack
            .scrub_committed_frames()
            .expect("scrub adopted pack");
        publish_or_validate_build_identity(&fixture.arguments, fixture.source_tip)
            .expect("publish adopted build identity");
        let identity = read_build_identity(&pack_path).expect("read adopted build identity");
        assert_eq!(
            identity.network_magic,
            formatted_network_magic(fixture.arguments.network_magic)
        );
        assert_ne!(identity.network_magic, "0xDEADBEEF");

        let report = CheckpointReport {
            schema_version: CHECKPOINT_SCHEMA_VERSION,
            authoritative_ready: false,
            complete: true,
            source_backend: "mdbx",
            source_namespace: STATE_SERVICE_NAMESPACE,
            network_magic: identity.network_magic,
            source_height: fixture.source_tip.height,
            source_root: displayed_root(fixture.source_tip.root_internal),
            source_root_internal_bytes: format!(
                "0x{}",
                hex::encode(fixture.source_tip.root_internal)
            ),
            source_namespace_sha256: format!("0x{}", hex::encode(namespace_digest)),
            rows: fixture.operations.len() as u64,
            resumed_rows: fixture.operations.len() as u64,
            value_bytes,
            frames: 2,
            rows_per_frame: fixture.arguments.rows_per_frame,
            pack_bytes,
            live_index_bytes,
            live_runs,
            decoded_index_memory_bytes,
            gc_runs_deleted: 0,
            gc_manifests_deleted: 0,
            gc_bytes_reclaimed: 0,
            pack_segment_format_version: PACK_SEGMENT_FORMAT_VERSION,
            pack_frame_format_version: PACK_FRAME_FORMAT_VERSION,
            pack_index_format_version: PACK_INDEX_FORMAT_VERSION,
            pack_manifest_format_version: PACK_MANIFEST_FORMAT_VERSION,
            tip_epoch: tip.epoch,
            tip_segment_id: tip.segment_id.get(),
            tip_frame_end: tip.frame_end,
            tip_payload_sha256: format!("0x{}", hex::encode(tip.payload_sha256)),
            scrubbed_frames: scrub.frames,
            scrubbed_rows: scrub.rows,
            scrubbed_puts: scrub.puts,
            scrubbed_tombstones: scrub.tombstones,
            scrubbed_payload_bytes: scrub.payload_bytes,
            scrubbed_value_bytes: scrub.value_bytes,
            scrub_elapsed_seconds: 0.5,
            elapsed_seconds: 1.0,
        };
        publish_after_reopen(&fixture.arguments, &report, opened.pack, true)
            .expect("replace legacy checkpoint with v2 marker");
        let upgraded: Value = serde_json::from_slice(
            &fs::read(pack_path.join("checkpoint.json")).expect("read upgraded checkpoint"),
        )
        .expect("decode upgraded checkpoint");
        assert_eq!(upgraded["schema_version"], CHECKPOINT_SCHEMA_VERSION);
        assert_eq!(upgraded["network_magic"], "0x334F454E");
        assert_eq!(upgraded["tip_epoch"], tip.epoch);
        assert_eq!(upgraded["tip_segment_id"], tip.segment_id.get());
        assert_eq!(
            upgraded["pack_segment_format_version"],
            PACK_SEGMENT_FORMAT_VERSION
        );
        assert_eq!(
            upgraded["tip_payload_sha256"],
            format!("0x{}", hex::encode(tip.payload_sha256))
        );
    }

    #[test]
    fn legacy_adoption_rejects_incomplete_root_and_frame_claims() {
        let temporary = tempdir().expect("temporary checkpoint parent");
        let incomplete_path = temporary.path().join("incomplete");
        let mut incomplete = create_legacy_fixture(&incomplete_path);
        incomplete.marker["complete"] = json!(false);
        write_legacy_marker(&incomplete_path, &incomplete.marker);
        let error = open_or_resume_pack(&incomplete.arguments, incomplete.source_tip)
            .err()
            .expect("incomplete legacy marker must reject adoption");
        assert!(error.to_string().contains("not a complete namespace"));

        let root_path = temporary.path().join("root-mismatch");
        let root_mismatch = create_legacy_fixture(&root_path);
        write_legacy_marker(&root_path, &root_mismatch.marker);
        let changed_tip = SourceTip {
            root_internal: [0x77; 32],
            ..root_mismatch.source_tip
        };
        let error = open_or_resume_pack(&root_mismatch.arguments, changed_tip)
            .err()
            .expect("source root mismatch must reject adoption");
        assert!(error.to_string().contains("displayed root differs"));

        let frame_path = temporary.path().join("frame-mismatch");
        let mut frame_mismatch = create_legacy_fixture(&frame_path);
        frame_mismatch.marker["frames"] = json!(3);
        write_legacy_marker(&frame_path, &frame_mismatch.marker);
        let error = open_or_resume_pack(&frame_mismatch.arguments, frame_mismatch.source_tip)
            .err()
            .expect("frame mismatch must reject adoption");
        assert!(error.to_string().contains("frame count differs"));

        let tip_path = temporary.path().join("tip-mismatch");
        let mut tip_mismatch = create_legacy_fixture(&tip_path);
        tip_mismatch.marker["tip_epoch"] = json!(tip_mismatch.tip.epoch + 1);
        tip_mismatch.marker["tip_frame_end"] = json!(tip_mismatch.tip.frame_end);
        tip_mismatch.marker["tip_payload_sha256"] = json!(format!(
            "0x{}",
            hex::encode(tip_mismatch.tip.payload_sha256)
        ));
        write_legacy_marker(&tip_path, &tip_mismatch.marker);
        let error = open_or_resume_pack(&tip_mismatch.arguments, tip_mismatch.source_tip)
            .err()
            .expect("declared tip mismatch must reject adoption");
        assert!(error.to_string().contains("tip identity differs"));
    }

    #[test]
    fn legacy_adoption_rejects_recomputed_digest_before_identity_publication() {
        let temporary = tempdir().expect("temporary checkpoint parent");
        let pack_path = temporary.path().join("pack");
        let mut fixture = create_legacy_fixture(&pack_path);
        fixture.marker["source_namespace_sha256"] = json!(format!("0x{}", hex::encode([0x99; 32])));
        write_legacy_marker(&pack_path, &fixture.marker);
        let opened = open_or_resume_pack(&fixture.arguments, fixture.source_tip)
            .expect("static legacy evidence remains structurally valid");
        let mut exact_rows = fixture.operations.clone();
        validate_existing_rows(&opened.pack, &mut exact_rows)
            .expect("legacy values still match exactly");
        let (digest, value_bytes) = namespace_evidence(&fixture.operations);
        let error = validate_legacy_scan_evidence(
            opened.legacy.as_ref().expect("legacy evidence"),
            fixture.operations.len() as u64,
            value_bytes,
            digest,
        )
        .expect_err("recomputed namespace digest must reject adoption");
        assert!(error.to_string().contains("namespace digest differs"));
        assert!(!pack_path.join(BUILD_IDENTITY_FILE).exists());
    }

    #[test]
    fn legacy_adoption_rejects_corrupt_tip_payload() {
        let temporary = tempdir().expect("temporary checkpoint parent");
        let pack_path = temporary.path().join("pack");
        let fixture = create_legacy_fixture(&pack_path);
        write_legacy_marker(&pack_path, &fixture.marker);
        let payload_start = fixture.tip.frame_end - fixture.tip.payload_bytes;
        let corrupt_offset = payload_start + 38;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(pack_path.join(fixture.tip.segment_id.file_name()))
            .expect("open legacy pack for corruption");
        file.seek(SeekFrom::Start(corrupt_offset))
            .expect("seek legacy value byte");
        let mut byte = [0u8; 1];
        file.read_exact(&mut byte).expect("read legacy value byte");
        byte[0] ^= 0xff;
        file.seek(SeekFrom::Start(corrupt_offset))
            .expect("rewind legacy value byte");
        file.write_all(&byte).expect("corrupt legacy value byte");
        file.sync_data().expect("sync corrupt legacy payload");
        drop(file);

        let error = open_or_resume_pack(&fixture.arguments, fixture.source_tip)
            .err()
            .expect("corrupt tip payload must reject adoption");
        assert!(format!("{error:#}").contains("checksum"));
        assert!(!pack_path.join(BUILD_IDENTITY_FILE).exists());
    }

    #[test]
    fn ordinary_resume_rejects_a_legacy_partial_pack_without_identity() {
        let temporary = tempdir().expect("temporary checkpoint parent");
        let pack_path = temporary.path().join("pack");
        let mut pack = PackStore::create(
            &pack_path,
            pack_store_config(1024 * 1024).expect("valid legacy partial configuration"),
        )
        .expect("create legacy partial");
        let mut key = [1u8; 33];
        key[0] = STATE_NODE_PREFIX;
        pack.append(&[PackOperation {
            key,
            kind: PackOpKind::Put(b"value".to_vec()),
        }])
        .expect("append legacy partial frame");
        drop(pack);
        let arguments = Arguments {
            network_magic: 0x334F_454E,
            mdbx_path: PathBuf::new(),
            pack_path,
            rows_per_frame: 1,
            max_rows: None,
            max_index_memory_bytes: 1024 * 1024,
            adopt_legacy_complete_pack: false,
            adopt_legacy_partial_pack: false,
        };
        let error = open_or_resume_pack(
            &arguments,
            SourceTip {
                height: 1,
                root_internal: [0x11; 32],
            },
        )
        .err()
        .expect("ordinary resume must reject missing identity");
        assert!(format!("{error:#}").contains("legacy partial packs must be rebuilt"));
    }

    #[test]
    fn explicit_legacy_partial_adoption_validates_full_frames_before_identity() {
        let temporary = tempdir().expect("temporary checkpoint parent");
        let pack_path = temporary.path().join("pack");
        let operations = (1u8..=4)
            .map(|suffix| {
                let mut key = [suffix; 33];
                key[0] = STATE_NODE_PREFIX;
                PackOperation {
                    key,
                    kind: PackOpKind::Put(vec![suffix; 3]),
                }
            })
            .collect::<Vec<_>>();
        let mut pack = PackStore::create(
            &pack_path,
            pack_store_config(1024 * 1024).expect("valid legacy partial configuration"),
        )
        .expect("create legacy partial pack");
        pack.append(&operations[..2]).expect("append first frame");
        pack.append(&operations[2..]).expect("append second frame");
        drop(pack);
        let arguments = Arguments {
            network_magic: 0x334F_454E,
            mdbx_path: PathBuf::new(),
            pack_path: pack_path.clone(),
            rows_per_frame: 2,
            max_rows: None,
            max_index_memory_bytes: 1024 * 1024,
            adopt_legacy_complete_pack: false,
            adopt_legacy_partial_pack: true,
        };
        let source_tip = SourceTip {
            height: 456,
            root_internal: [0x66; 32],
        };
        let mut opened = open_or_resume_pack(&arguments, source_tip)
            .expect("open explicit legacy partial adoption");
        assert_eq!((opened.resumed_rows, opened.frames), (4, 2));
        assert_eq!(opened.publish_identity_after_rows, Some(4));
        assert!(!pack_path.join(BUILD_IDENTITY_FILE).exists());

        let mut first = operations[..2].to_vec();
        validate_existing_rows(&opened.pack, &mut first).expect("validate first prefix frame");
        assert!(!pack_path.join(BUILD_IDENTITY_FILE).exists());
        let mut second = operations[2..].to_vec();
        validate_existing_rows(&opened.pack, &mut second).expect("validate final prefix frame");
        publish_adopted_partial_identity(&mut opened.pack, &arguments, source_tip)
            .expect("publish identity after complete prefix validation");
        let identity = read_build_identity(&pack_path).expect("read adopted identity");
        assert_eq!(identity.source_height, source_tip.height);
        assert_eq!(identity.rows_per_frame, 2);
    }

    #[test]
    fn legacy_partial_adoption_rejects_non_full_frame_geometry() {
        let temporary = tempdir().expect("temporary checkpoint parent");
        let pack_path = temporary.path().join("pack");
        let mut key = [7u8; 33];
        key[0] = STATE_NODE_PREFIX;
        let mut pack = PackStore::create(
            &pack_path,
            pack_store_config(1024 * 1024).expect("valid legacy partial configuration"),
        )
        .expect("create legacy partial pack");
        pack.append(&[PackOperation {
            key,
            kind: PackOpKind::Put(b"value".to_vec()),
        }])
        .expect("append short frame");
        drop(pack);
        let arguments = Arguments {
            network_magic: 0x334F_454E,
            mdbx_path: PathBuf::new(),
            pack_path,
            rows_per_frame: 2,
            max_rows: None,
            max_index_memory_bytes: 1024 * 1024,
            adopt_legacy_complete_pack: false,
            adopt_legacy_partial_pack: true,
        };
        let error = open_or_resume_pack(
            &arguments,
            SourceTip {
                height: 1,
                root_internal: [0x11; 32],
            },
        )
        .err()
        .expect("non-full legacy prefix frame must be rejected");
        assert!(error.to_string().contains("not exactly 2 rows per frame"));
        assert!(!arguments.pack_path.join(BUILD_IDENTITY_FILE).exists());
    }

    #[test]
    fn incomplete_report_cannot_be_confused_with_a_complete_checkpoint() {
        let report = CheckpointReport {
            schema_version: CHECKPOINT_SCHEMA_VERSION,
            authoritative_ready: false,
            complete: false,
            source_backend: "mdbx",
            source_namespace: STATE_SERVICE_NAMESPACE,
            network_magic: "0x334F454E".to_owned(),
            source_height: 1,
            source_root: displayed_root([0u8; 32]),
            source_root_internal_bytes: format!("0x{}", hex::encode([0u8; 32])),
            source_namespace_sha256: format!("0x{}", hex::encode([0u8; 32])),
            rows: 10,
            resumed_rows: 0,
            value_bytes: 20,
            frames: 1,
            rows_per_frame: 10,
            pack_bytes: 30,
            live_index_bytes: 10,
            live_runs: 1,
            decoded_index_memory_bytes: 2,
            gc_runs_deleted: 0,
            gc_manifests_deleted: 0,
            gc_bytes_reclaimed: 0,
            pack_segment_format_version: PACK_SEGMENT_FORMAT_VERSION,
            pack_frame_format_version: PACK_FRAME_FORMAT_VERSION,
            pack_index_format_version: PACK_INDEX_FORMAT_VERSION,
            pack_manifest_format_version: PACK_MANIFEST_FORMAT_VERSION,
            tip_epoch: 0,
            tip_segment_id: PackSegmentId::INITIAL.get(),
            tip_frame_end: 30,
            tip_payload_sha256: format!("0x{}", hex::encode([0u8; 32])),
            scrubbed_frames: 1,
            scrubbed_rows: 10,
            scrubbed_puts: 10,
            scrubbed_tombstones: 0,
            scrubbed_payload_bytes: 20,
            scrubbed_value_bytes: 20,
            scrub_elapsed_seconds: 0.5,
            elapsed_seconds: 1.0,
        };
        let json = serde_json::to_value(report).expect("serialize checkpoint report");
        assert_eq!(json["complete"], false);
        assert_eq!(json["authoritative_ready"], false);
        assert_eq!(json["schema_version"], CHECKPOINT_SCHEMA_VERSION);
        assert_eq!(json["network_magic"], "0x334F454E");
        assert_eq!(
            json["pack_segment_format_version"],
            PACK_SEGMENT_FORMAT_VERSION
        );
        assert_eq!(json["pack_frame_format_version"], PACK_FRAME_FORMAT_VERSION);
        assert_eq!(json["pack_index_format_version"], PACK_INDEX_FORMAT_VERSION);
        assert_eq!(
            json["pack_manifest_format_version"],
            PACK_MANIFEST_FORMAT_VERSION
        );
        assert_eq!(json["tip_segment_id"], PackSegmentId::INITIAL.get());
    }

    #[test]
    fn partial_pack_resume_revalidates_exact_existing_prefix_values() {
        let temporary = tempdir().expect("temporary checkpoint parent");
        let pack_path = temporary.path().join("pack");
        let mut first_key = [1u8; 33];
        first_key[0] = STATE_NODE_PREFIX;
        let mut second_key = [2u8; 33];
        second_key[0] = STATE_NODE_PREFIX;
        let original = vec![
            PackOperation {
                key: first_key,
                kind: PackOpKind::Put(b"first".to_vec()),
            },
            PackOperation {
                key: second_key,
                kind: PackOpKind::Put(b"second".to_vec()),
            },
        ];
        let arguments = Arguments {
            network_magic: 0x334F_454E,
            mdbx_path: PathBuf::new(),
            pack_path: pack_path.clone(),
            rows_per_frame: 2,
            max_rows: None,
            max_index_memory_bytes: 1024 * 1024,
            adopt_legacy_complete_pack: false,
            adopt_legacy_partial_pack: false,
        };
        let source_tip = SourceTip {
            height: 123,
            root_internal: [0x55; 32],
        };
        let opened = open_or_resume_pack(&arguments, source_tip).expect("create partial pack");
        assert_eq!((opened.resumed_rows, opened.frames), (0, 0));
        let mut store = opened.pack;
        store.append(&original).expect("append partial prefix");
        drop(store);

        let opened = open_or_resume_pack(&arguments, source_tip).expect("open partial pack");
        assert_eq!((opened.resumed_rows, opened.frames), (2, 1));
        let pack = opened.pack;
        let mut matching = original.clone();
        validate_existing_rows(&pack, &mut matching).expect("validate matching prefix");
        assert!(matching.is_empty());

        let mut mismatching = original;
        mismatching[1].kind = PackOpKind::Put(b"changed".to_vec());
        let error = validate_existing_rows(&pack, &mut mismatching)
            .expect_err("changed source value must reject resume");
        assert!(error.to_string().contains("differs from the source"));
    }

    #[test]
    fn partial_pack_resume_rejects_identity_and_format_drift() {
        let temporary = tempdir().expect("temporary checkpoint parent");
        let pack_path = temporary.path().join("pack");
        let arguments = Arguments {
            network_magic: 0x334F_454E,
            mdbx_path: PathBuf::new(),
            pack_path: pack_path.clone(),
            rows_per_frame: 1,
            max_rows: None,
            max_index_memory_bytes: 1024 * 1024,
            adopt_legacy_complete_pack: false,
            adopt_legacy_partial_pack: false,
        };
        let source_tip = SourceTip {
            height: 123,
            root_internal: [0x55; 32],
        };
        let mut store = open_or_resume_pack(&arguments, source_tip)
            .expect("create partial pack")
            .pack;
        let mut key = [1u8; 33];
        key[0] = STATE_NODE_PREFIX;
        store
            .append(&[PackOperation {
                key,
                kind: PackOpKind::Put(b"value".to_vec()),
            }])
            .expect("append partial prefix");
        drop(store);

        let mut wrong_network = arguments.clone();
        wrong_network.network_magic = 0x3554_334E;
        let error = open_or_resume_pack(&wrong_network, source_tip)
            .err()
            .expect("network identity drift must reject resume");
        assert!(error.to_string().contains("network magic changed"));

        let changed_tip = SourceTip {
            height: source_tip.height + 1,
            ..source_tip
        };
        let error = open_or_resume_pack(&arguments, changed_tip)
            .err()
            .expect("source generation drift must reject resume");
        assert!(error.to_string().contains("source height or root changed"));

        let identity_path = pack_path.join(BUILD_IDENTITY_FILE);
        let mut identity = read_build_identity(&pack_path).expect("read build identity");
        identity.pack_index_format_version = identity.pack_index_format_version.saturating_add(1);
        fs::write(
            &identity_path,
            serde_json::to_vec_pretty(&identity).expect("encode changed identity"),
        )
        .expect("replace build identity");
        let error = open_or_resume_pack(&arguments, source_tip)
            .err()
            .expect("format identity drift must reject resume");
        assert!(error.to_string().contains("format version changed"));
    }

    #[test]
    fn partial_pack_resume_rejects_inconsistent_frame_geometry() {
        let temporary = tempdir().expect("temporary checkpoint parent");
        let pack_path = temporary.path().join("pack");
        let mut arguments = Arguments {
            network_magic: 0x334F_454E,
            mdbx_path: PathBuf::new(),
            pack_path: pack_path.clone(),
            rows_per_frame: 1,
            max_rows: None,
            max_index_memory_bytes: 1024 * 1024,
            adopt_legacy_complete_pack: false,
            adopt_legacy_partial_pack: false,
        };
        let source_tip = SourceTip {
            height: 123,
            root_internal: [0x55; 32],
        };
        let mut store = open_or_resume_pack(&arguments, source_tip)
            .expect("create partial pack")
            .pack;
        for suffix in [1u8, 2] {
            let mut key = [suffix; 33];
            key[0] = STATE_NODE_PREFIX;
            store
                .append(&[PackOperation {
                    key,
                    kind: PackOpKind::Put(vec![suffix]),
                }])
                .expect("append one complete frame");
        }
        drop(store);

        arguments.rows_per_frame = 2;
        let mut identity = read_build_identity(&pack_path).expect("read build identity");
        identity.rows_per_frame = arguments.rows_per_frame;
        fs::write(
            pack_path.join(BUILD_IDENTITY_FILE),
            serde_json::to_vec_pretty(&identity).expect("encode changed identity"),
        )
        .expect("replace build identity");
        let error = open_or_resume_pack(&arguments, source_tip)
            .err()
            .expect("frame geometry inconsistent with declared size must reject resume");
        assert!(error.to_string().contains("not exactly 2 rows per frame"));
    }

    #[test]
    fn network_magic_parser_accepts_decimal_and_hex() {
        assert_eq!(
            parse_u32_literal("860833102").expect("decimal"),
            0x334F_454E
        );
        assert_eq!(parse_u32_literal("0x334F454E").expect("hex"), 0x334F_454E);
        assert!(parse_u32_literal("0x100000000").is_err());
    }

    #[test]
    fn command_line_requires_and_preserves_network_magic() {
        let arguments = parse_arguments_from(
            [
                "--network-magic",
                "0x334F454E",
                "--mdbx",
                "/source",
                "--pack",
                "/checkpoint",
                "--adopt-legacy-complete-pack",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .expect("parse complete command line");
        assert_eq!(arguments.network_magic, 0x334F_454E);
        assert_eq!(arguments.mdbx_path, Path::new("/source"));
        assert_eq!(arguments.pack_path, Path::new("/checkpoint"));
        assert!(arguments.adopt_legacy_complete_pack);
        assert!(!arguments.adopt_legacy_partial_pack);

        let partial = parse_arguments_from(
            [
                "--network-magic",
                "0x334F454E",
                "--mdbx",
                "/source",
                "--pack",
                "/checkpoint",
                "--adopt-legacy-partial-pack",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .expect("parse partial adoption command line");
        assert!(partial.adopt_legacy_partial_pack);
        assert!(!partial.adopt_legacy_complete_pack);

        let error = parse_arguments_from(
            ["--mdbx", "/source", "--pack", "/checkpoint"]
                .into_iter()
                .map(str::to_owned),
        )
        .expect_err("network magic must be explicit");
        assert!(error.to_string().contains("--network-magic is required"));

        let error = parse_arguments_from(
            [
                "--network-magic",
                "0x334F454E",
                "--mdbx",
                "/source",
                "--pack",
                "/checkpoint",
                "--max-rows",
                "1",
                "--adopt-legacy-complete-pack",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .expect_err("legacy adoption cannot use a capped source scan");
        assert!(
            error
                .to_string()
                .contains("cannot be combined with --max-rows")
        );

        let error = parse_arguments_from(
            [
                "--network-magic",
                "0x334F454E",
                "--mdbx",
                "/source",
                "--pack",
                "/checkpoint",
                "--adopt-legacy-complete-pack",
                "--adopt-legacy-partial-pack",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .expect_err("legacy adoption modes must be mutually exclusive");
        assert!(error.to_string().contains("mutually exclusive"));
    }
}
