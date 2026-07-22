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

use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};
use neo_crypto::Sha256Hasher;
use neo_state_packs::{
    CHECKPOINT_NAMESPACE_DIGEST_DOMAIN, PACK_FRAME_FORMAT_VERSION, PACK_INDEX_FORMAT_VERSION,
    PACK_MANIFEST_FORMAT_VERSION, PACK_SEGMENT_FORMAT_VERSION, PackFrameContext, PackOpKind,
    PackOperation, PackSegmentId, PackStore, PackStoreConfig,
};
use neo_state_service::read_current_local_root;
use neo_storage::persistence::StoreFactory;
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::storage::StorageConfig;
use neo_storage::{StorageError, StorageResult};
use serde::{Deserialize, Serialize};

const STATE_NODE_PREFIX: u8 = 0xf0;
const STATE_SERVICE_NAMESPACE: &str = "neo_state_service";
const DEFAULT_ROWS_PER_FRAME: usize = 1_000_000;
const DEFAULT_MAX_INDEX_MEMORY_MB: u64 = 512;
const CHECKPOINT_SCHEMA_VERSION: u32 = 4;
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
    tip_frame_sha256: String,
    scrubbed_frames: u64,
    scrubbed_rows: u64,
    scrubbed_puts: u64,
    scrubbed_tombstones: u64,
    scrubbed_payload_bytes: u64,
    scrubbed_value_bytes: u64,
    scrub_elapsed_seconds: f64,
    elapsed_seconds: f64,
}

struct OpenedCheckpoint {
    pack: PackStore,
    resumed_rows: u64,
    frames: u64,
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
                } else {
                    append_frame(&mut pack, source_tip, &mut operations)?;
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
        rows >= resumed_rows,
        "partial checkpoint contains more rows than the source namespace"
    );
    if !operations.is_empty() {
        if rows <= resumed_rows {
            validate_existing_rows(&pack, &mut operations)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        } else {
            append_frame(&mut pack, source_tip, &mut operations)
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
    let complete = arguments.max_rows.is_none();
    let report = CheckpointReport {
        schema_version: CHECKPOINT_SCHEMA_VERSION,
        // The report is published only by `publish_after_reopen`, after the
        // complete source scan, stable height/root check, payload scrub, tip
        // frame-digest verification, and pack reopen all succeed. Bounded builds
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
        tip_frame_sha256: format!("0x{}", hex::encode(tip.frame_sha256)),
        scrubbed_frames: scrub.frames,
        scrubbed_rows: scrub.rows,
        scrubbed_puts: scrub.puts,
        scrubbed_tombstones: scrub.tombstones,
        scrubbed_payload_bytes: scrub.payload_bytes,
        scrubbed_value_bytes: scrub.value_bytes,
        scrub_elapsed_seconds,
        elapsed_seconds: started.elapsed().as_secs_f64(),
    };
    publish_after_reopen(&arguments, &report, pack)?;
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
            other => bail!("unknown argument {other}"),
        }
    }
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
    let root = read_current_local_root(store).context("read frozen StateService source tip")?;
    Ok(SourceTip {
        height: root.index(),
        root_internal: root.root_hash().to_array(),
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
    })
}

fn read_build_identity(pack_path: &Path) -> Result<BuildIdentity> {
    let path = pack_path.join(BUILD_IDENTITY_FILE);
    let file = File::open(&path).with_context(|| {
        format!(
            "open current checkpoint build identity {}; stores without it must be rebuilt",
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

fn checkpoint_frame_context(source_tip: SourceTip) -> PackFrameContext {
    PackFrameContext::new(
        source_tip.height,
        source_tip.height,
        source_tip.root_internal,
        source_tip.root_internal,
    )
}

fn append_frame(
    pack: &mut PackStore,
    source_tip: SourceTip,
    operations: &mut Vec<PackOperation>,
) -> StorageResult<()> {
    pack.append_frame(checkpoint_frame_context(source_tip), operations)
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
    ensure!(
        reopened_tip.epoch == report.tip_epoch
            && reopened_tip.segment_id.get() == report.tip_segment_id
            && reopened_tip.frame_end == report.tip_frame_end
            && format!("0x{}", hex::encode(reopened_tip.frame_sha256)) == report.tip_frame_sha256,
        "reopened checkpoint exposes a different tip frame"
    );
    drop(reopened);

    let checkpoint = arguments.pack_path.join("checkpoint.json");
    ensure!(
        !checkpoint.exists(),
        "checkpoint marker appeared before publication"
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
    use tempfile::tempdir;

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
            tip_frame_sha256: format!("0x{}", hex::encode([0u8; 32])),
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
        };
        let source_tip = SourceTip {
            height: 123,
            root_internal: [0x55; 32],
        };
        let opened = open_or_resume_pack(&arguments, source_tip).expect("create partial pack");
        assert_eq!((opened.resumed_rows, opened.frames), (0, 0));
        let mut store = opened.pack;
        store
            .append_frame(checkpoint_frame_context(source_tip), &original)
            .expect("append partial prefix");
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
            .append_frame(
                checkpoint_frame_context(source_tip),
                &[PackOperation {
                    key,
                    kind: PackOpKind::Put(b"value".to_vec()),
                }],
            )
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
                .append_frame(
                    checkpoint_frame_context(source_tip),
                    &[PackOperation {
                        key,
                        kind: PackOpKind::Put(vec![suffix]),
                    }],
                )
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
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .expect("parse complete command line");
        assert_eq!(arguments.network_magic, 0x334F_454E);
        assert_eq!(arguments.mdbx_path, Path::new("/source"));
        assert_eq!(arguments.pack_path, Path::new("/checkpoint"));

        let error = parse_arguments_from(
            ["--mdbx", "/source", "--pack", "/checkpoint"]
                .into_iter()
                .map(str::to_owned),
        )
        .expect_err("network magic must be explicit");
        assert!(error.to_string().contains("--network-magic is required"));
    }
}
