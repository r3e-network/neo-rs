//! Build a bounded offline node-pack checkpoint from either the authoritative
//! MDBX StateService `0xf0 || node_hash` namespace or an authenticated neutral
//! migration stream.
//!
//! Both source modes require read-only coordinated MDBX metadata. A checkpoint
//! marker is published only after the source height/root remain stable, the
//! complete namespace scrubs exactly, its root node resolves, and the pack
//! reopens successfully. Interrupted or bounded smoke builds therefore never
//! look like complete authoritative checkpoints.
//!
//! Usage:
//!   neo-pack-build --network-magic <u32-or-hex>
//!     --mdbx <canonical-store-dir> --pack <new-pack-dir>
//!     [--rows-per-frame N] [--max-rows N] [--max-index-memory-mb N]
//!   neo-pack-build --network-magic <u32-or-hex>
//!     --migration-stream <neutral-v1-file>
//!     --metadata-mdbx <canonical-store-dir> --pack <new-pack-dir>
//!     [--rows-per-frame N] [--max-index-memory-mb N]

use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};
use neo_crypto::Sha256Hasher;
use neo_crypto::mpt_trie::Node;
use neo_primitives::UInt256;
use neo_state_packs::checkpoint::{
    PACK_CHECKPOINT_SCHEMA_VERSION, PACK_CHECKPOINT_SOURCE_NAMESPACE,
    PackCheckpoint as CheckpointReport,
};
use neo_state_packs::migration::{
    MigrationStreamEvidence, MigrationStreamHeader, MigrationStreamLimits, MigrationStreamReader,
};
use neo_state_packs::{
    CHECKPOINT_NAMESPACE_DIGEST_DOMAIN, PACK_FRAME_FORMAT_VERSION, PACK_FRAME_ROW_METADATA_BYTES,
    PACK_INDEX_FORMAT_VERSION, PACK_MANIFEST_FORMAT_VERSION, PACK_SEGMENT_FORMAT_VERSION,
    PackFrameContext, PackOpKind, PackOperation, PackSegmentId, PackStore, PackStoreConfig,
};
use neo_state_service::read_current_local_root;
use neo_storage::persistence::StoreFactory;
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::storage::StorageConfig;
use neo_storage::{StorageError, StorageResult};
use serde::{Deserialize, Serialize};

const STATE_NODE_PREFIX: u8 = 0xf0;
const DEFAULT_ROWS_PER_FRAME: usize = 1_000_000;
const DEFAULT_MAX_INDEX_MEMORY_MB: u64 = 512;
const BUILD_IDENTITY_SCHEMA_VERSION: u32 = 3;
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
    source: SourceArguments,
    pack_path: PathBuf,
    rows_per_frame: usize,
    max_rows: Option<u64>,
    max_index_memory_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SourceArguments {
    Mdbx {
        path: PathBuf,
    },
    MigrationStream {
        stream_path: PathBuf,
        metadata_mdbx_path: PathBuf,
    },
}

impl SourceArguments {
    fn metadata_mdbx_path(&self) -> &Path {
        match self {
            Self::Mdbx { path } => path,
            Self::MigrationStream {
                metadata_mdbx_path, ..
            } => metadata_mdbx_path,
        }
    }
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
    source: BuildSourceIdentity,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "backend", rename_all = "kebab-case", deny_unknown_fields)]
enum BuildSourceIdentity {
    Mdbx,
    MigrationStreamV1 {
        rows: u64,
        value_bytes: u64,
        payload_bytes: u64,
        namespace_sha256: String,
        payload_sha256: String,
    },
}

impl BuildSourceIdentity {
    fn migration_stream(header: MigrationStreamHeader) -> Self {
        Self::MigrationStreamV1 {
            rows: header.rows,
            value_bytes: header.value_bytes,
            payload_bytes: header.payload_bytes,
            namespace_sha256: format!("0x{}", hex::encode(header.namespace_sha256)),
            payload_sha256: format!("0x{}", hex::encode(header.payload_sha256)),
        }
    }
}

impl BuildIdentity {
    fn current(arguments: &Arguments, source_tip: SourceTip, source: BuildSourceIdentity) -> Self {
        Self {
            schema_version: BUILD_IDENTITY_SCHEMA_VERSION,
            source,
            source_namespace: PACK_CHECKPOINT_SOURCE_NAMESPACE.to_owned(),
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

struct OpenedCheckpoint {
    pack: PackStore,
    resumed_rows: u64,
    frames: u64,
}

struct RowAccumulator<'a> {
    arguments: &'a Arguments,
    source_tip: SourceTip,
    pack: PackStore,
    resumed_rows: u64,
    frames: u64,
    operations: Vec<PackOperation>,
    frame_value_bytes: u64,
    rows: u64,
    value_bytes: u64,
    namespace_hasher: Sha256Hasher,
    max_frame_payload_bytes: u64,
}

struct AccumulatedCheckpoint {
    pack: PackStore,
    resumed_rows: u64,
    frames: u64,
    rows: u64,
    value_bytes: u64,
    namespace_sha256: [u8; 32],
}

fn main() -> Result<()> {
    let arguments = parse_arguments()?;
    let canonical: Arc<RuntimeStore> = StoreFactory::get_store_with_config(
        "mdbx",
        StorageConfig {
            path: arguments.source.metadata_mdbx_path().to_path_buf(),
            read_only: true,
            ..Default::default()
        },
    )
    .map_err(|error| anyhow::anyhow!("open MDBX store: {error}"))?;
    let state_store = canonical
        .open_coordinated_namespace(PACK_CHECKPOINT_SOURCE_NAMESPACE)
        .context("open coordinated MDBX StateService namespace")?;
    let source_tip = read_source_tip(&state_store)?;

    let started = Instant::now();
    let (mut migration_reader, build_source) = match &arguments.source {
        SourceArguments::Mdbx { .. } => (None, BuildSourceIdentity::Mdbx),
        SourceArguments::MigrationStream { stream_path, .. } => {
            let file = File::open(stream_path).with_context(|| {
                format!("open neutral migration stream {}", stream_path.display())
            })?;
            let reader =
                MigrationStreamReader::new(BufReader::new(file), MigrationStreamLimits::default())
                    .context("decode neutral migration stream header")?;
            validate_migration_header(reader.header(), &arguments, source_tip)?;
            // The header is available before any pack mutation and its two
            // digests bind the complete payload once `finish` validates it.
            // The trailer-only stream digest would require a separate prepass.
            let build_source = BuildSourceIdentity::migration_stream(reader.header());
            (Some(reader), build_source)
        }
    };
    let opened = open_or_resume_pack(&arguments, source_tip, build_source)?;
    let mut accumulator = RowAccumulator::new(&arguments, source_tip, opened)?;
    let migration_evidence = match migration_reader.as_mut() {
        None => {
            let visited = state_store.visit_raw_entries_with_prefix(
                &[STATE_NODE_PREFIX],
                arguments.max_rows,
                |key, value| accumulator.push_storage_row(key, value),
            )?;
            ensure!(
                visited == accumulator.rows,
                "streamed row count changed unexpectedly"
            );
            None
        }
        Some(reader) => {
            while let Some(row) = reader
                .read_row()
                .context("decode neutral migration stream row")?
            {
                accumulator.push(row.key, &row.value)?;
            }
            Some(
                migration_reader
                    .take()
                    .expect("migration source installs one reader")
                    .finish()
                    .context("validate neutral migration stream trailer")?,
            )
        }
    };
    let accumulated = accumulator.finish()?;
    if let Some(evidence) = migration_evidence {
        validate_migration_evidence(evidence, &accumulated)?;
    }

    let source_tip_after = read_source_tip(&state_store)?;
    ensure!(
        source_tip_after == source_tip,
        "source StateService height/root changed during checkpoint build"
    );
    finalize_checkpoint(&arguments, source_tip, accumulated, started)
}

fn finalize_checkpoint(
    arguments: &Arguments,
    source_tip: SourceTip,
    accumulated: AccumulatedCheckpoint,
    started: Instant,
) -> Result<()> {
    let AccumulatedCheckpoint {
        mut pack,
        resumed_rows,
        frames,
        rows,
        value_bytes,
        namespace_sha256,
    } = accumulated;
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
        checkpoint_evidence.sha256 == namespace_sha256,
        "checkpoint pack namespace digest does not match the frozen source"
    );
    let index_evidence = pack
        .checkpoint_index_evidence()
        .context("bind checkpoint materialized indexes to committed frame rows")?;
    ensure!(
        index_evidence.frame_records == rows
            && index_evidence.winner_records == rows
            && index_evidence.value_bytes == value_bytes,
        "checkpoint index-binding evidence differs from the frozen source geometry"
    );
    validate_root_node(&pack, source_tip)?;
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
        schema_version: PACK_CHECKPOINT_SCHEMA_VERSION,
        // The report is published only by `publish_after_reopen`, after the
        // complete source scan, stable height/root check, payload scrub, tip
        // frame-digest verification, and pack reopen all succeed. Bounded builds
        // remain explicitly ineligible.
        authoritative_ready: complete,
        complete,
        // The neutral stream is an authenticated transport of this exact
        // legacy MDBX namespace, not a distinct semantic source backend.
        source_backend: "mdbx".to_owned(),
        source_namespace: PACK_CHECKPOINT_SOURCE_NAMESPACE.to_owned(),
        network_magic: formatted_network_magic(arguments.network_magic),
        source_height: source_tip.height,
        source_root: displayed_root(source_tip.root_internal),
        source_root_internal_bytes: format!("0x{}", hex::encode(source_tip.root_internal)),
        source_namespace_sha256: format!("0x{}", hex::encode(namespace_sha256)),
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
    publish_after_reopen(arguments, &report, source_tip, pack)?;
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
    let mut migration_stream_path = None;
    let mut metadata_mdbx_path = None;
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
            "--migration-stream" => {
                migration_stream_path = arguments.next().map(PathBuf::from);
            }
            "--metadata-mdbx" => metadata_mdbx_path = arguments.next().map(PathBuf::from),
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
    ensure!(
        rows_per_frame <= PackStoreConfig::HARD_MAX_FRAME_ROWS as usize,
        "--rows-per-frame exceeds the pack format limit of {}",
        PackStoreConfig::HARD_MAX_FRAME_ROWS
    );
    let source = match (mdbx_path, migration_stream_path, metadata_mdbx_path) {
        (Some(path), None, None) => SourceArguments::Mdbx { path },
        (None, Some(stream_path), Some(metadata_mdbx_path)) => {
            ensure!(
                max_rows.is_none(),
                "--max-rows is not valid with --migration-stream because the complete authenticated stream is mandatory"
            );
            SourceArguments::MigrationStream {
                stream_path,
                metadata_mdbx_path,
            }
        }
        (None, None, _) => {
            bail!("choose --mdbx, or use --migration-stream together with --metadata-mdbx")
        }
        _ => bail!("--mdbx is mutually exclusive with --migration-stream and --metadata-mdbx"),
    };
    Ok(Arguments {
        network_magic: network_magic.context("--network-magic is required")?,
        source,
        pack_path: pack_path.context("--pack is required")?,
        rows_per_frame,
        max_rows,
        max_index_memory_bytes,
    })
}

impl<'a> RowAccumulator<'a> {
    fn new(
        arguments: &'a Arguments,
        source_tip: SourceTip,
        opened: OpenedCheckpoint,
    ) -> Result<Self> {
        let config = pack_store_config(arguments.max_index_memory_bytes)?;
        let mut operations = Vec::new();
        operations
            .try_reserve_exact(arguments.rows_per_frame)
            .context("reserve checkpoint frame operations")?;
        let mut namespace_hasher = Sha256Hasher::new();
        namespace_hasher.update(CHECKPOINT_NAMESPACE_DIGEST_DOMAIN);
        Ok(Self {
            arguments,
            source_tip,
            pack: opened.pack,
            resumed_rows: opened.resumed_rows,
            frames: opened.frames,
            operations,
            frame_value_bytes: 0,
            rows: 0,
            value_bytes: 0,
            namespace_hasher,
            max_frame_payload_bytes: config.max_frame_payload_bytes(),
        })
    }

    fn push_storage_row(&mut self, key: &[u8], value: &[u8]) -> StorageResult<()> {
        ensure_storage(
            key.len() == 33 && key.first() == Some(&STATE_NODE_PREFIX),
            "StateService node scan returned a malformed key",
        )?;
        let key: [u8; 33] = key.try_into().expect("validated 33-byte key");
        self.push(key, value)
            .map_err(|error| StorageError::Backend {
                message: format!("build checkpoint pack: {error:#}"),
            })
    }

    fn push(&mut self, key: [u8; 33], value: &[u8]) -> Result<()> {
        ensure!(
            key[0] == STATE_NODE_PREFIX,
            "checkpoint row is outside the exact StateService node namespace"
        );
        let expected_hash = UInt256::from_bytes(&key[1..])
            .context("decode checkpoint MPT node hash from its storage key")?;
        Node::validate_persisted(value, expected_hash)
            .context("validate checkpoint MPT node row")?;
        let value_len =
            u64::try_from(value.len()).context("checkpoint value length overflows u64")?;
        if !self.operations.is_empty()
            && (self.operations.len() == self.arguments.rows_per_frame
                || !self.next_row_fits(value_len)?)
        {
            self.flush()?;
        }
        ensure!(
            self.next_row_fits(value_len)?,
            "one checkpoint row cannot fit the configured pack frame payload bound"
        );

        let mut owned_value = Vec::new();
        owned_value
            .try_reserve_exact(value.len())
            .context("reserve exact checkpoint node value")?;
        owned_value.extend_from_slice(value);
        self.namespace_hasher
            .update(&(key.len() as u32).to_le_bytes());
        self.namespace_hasher.update(&key);
        self.namespace_hasher.update(&value_len.to_le_bytes());
        self.namespace_hasher.update(value);
        self.rows = self
            .rows
            .checked_add(1)
            .context("checkpoint row count overflows u64")?;
        self.value_bytes = self
            .value_bytes
            .checked_add(value_len)
            .context("checkpoint value byte count overflows u64")?;
        self.frame_value_bytes = self
            .frame_value_bytes
            .checked_add(value_len)
            .context("checkpoint frame value byte count overflows u64")?;
        self.operations.push(PackOperation {
            key,
            kind: PackOpKind::Put(owned_value),
        });
        Ok(())
    }

    fn next_row_fits(&self, value_len: u64) -> Result<bool> {
        let rows = u64::try_from(self.operations.len())
            .context("checkpoint frame row count overflows u64")?
            .checked_add(1)
            .context("checkpoint frame row count overflows u64")?;
        let metadata_bytes = rows
            .checked_mul(PACK_FRAME_ROW_METADATA_BYTES as u64)
            .context("checkpoint frame metadata length overflows u64")?;
        let value_bytes = self
            .frame_value_bytes
            .checked_add(value_len)
            .context("checkpoint frame value length overflows u64")?;
        Ok(metadata_bytes
            .checked_add(value_bytes)
            .context("checkpoint frame payload length overflows u64")?
            <= self.max_frame_payload_bytes)
    }

    fn flush(&mut self) -> Result<()> {
        if self.operations.is_empty() {
            return Ok(());
        }
        let frame_rows = u64::try_from(self.operations.len())
            .context("checkpoint frame row count overflows u64")?;
        let frame_start = self
            .rows
            .checked_sub(frame_rows)
            .context("checkpoint frame row accounting underflow")?;
        if self.rows <= self.resumed_rows {
            validate_existing_rows(&self.pack, &mut self.operations)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        } else {
            ensure!(
                frame_start >= self.resumed_rows,
                "partial checkpoint row horizon falls inside a deterministically rebuilt frame"
            );
            append_frame(&mut self.pack, self.source_tip, &mut self.operations)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            self.frames = self
                .frames
                .checked_add(1)
                .context("checkpoint frame count overflows u64")?;
        }
        self.frame_value_bytes = 0;
        eprintln!(
            "checkpoint progress: rows={} frames={} value_bytes={}",
            self.rows, self.frames, self.value_bytes
        );
        Ok(())
    }

    fn finish(mut self) -> Result<AccumulatedCheckpoint> {
        self.flush()?;
        ensure!(self.rows > 0, "StateService node namespace is empty");
        ensure!(
            self.rows >= self.resumed_rows,
            "partial checkpoint contains more rows than the source namespace"
        );
        Ok(AccumulatedCheckpoint {
            pack: self.pack,
            resumed_rows: self.resumed_rows,
            frames: self.frames,
            rows: self.rows,
            value_bytes: self.value_bytes,
            namespace_sha256: self.namespace_hasher.finalize(),
        })
    }
}

fn validate_migration_header(
    header: MigrationStreamHeader,
    arguments: &Arguments,
    source_tip: SourceTip,
) -> Result<()> {
    ensure!(
        header.network_magic == arguments.network_magic,
        "migration stream network 0x{:08X} differs from --network-magic 0x{:08X}",
        header.network_magic,
        arguments.network_magic
    );
    ensure!(
        (header.height, header.root_internal) == (source_tip.height, source_tip.root_internal),
        "migration stream height/root differs from coordinated StateService metadata"
    );
    ensure!(header.rows > 0, "migration stream node namespace is empty");
    Ok(())
}

fn validate_migration_evidence(
    evidence: MigrationStreamEvidence,
    accumulated: &AccumulatedCheckpoint,
) -> Result<()> {
    ensure!(
        evidence.header.rows == accumulated.rows
            && evidence.header.value_bytes == accumulated.value_bytes,
        "migration stream geometry differs from the imported checkpoint"
    );
    ensure!(
        evidence.header.namespace_sha256 == accumulated.namespace_sha256,
        "migration stream namespace digest differs from the imported checkpoint"
    );
    Ok(())
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

fn open_or_resume_pack(
    arguments: &Arguments,
    source_tip: SourceTip,
    source: BuildSourceIdentity,
) -> Result<OpenedCheckpoint> {
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
    let expected_identity = BuildIdentity::current(arguments, source_tip, source);
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
        actual.source == expected.source && actual.source_namespace == expected.source_namespace,
        "checkpoint build source identity or namespace changed"
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

fn validate_root_node(pack: &PackStore, source_tip: SourceTip) -> Result<()> {
    let mut root_key = [0u8; 33];
    root_key[0] = STATE_NODE_PREFIX;
    root_key[1..].copy_from_slice(&source_tip.root_internal);
    ensure!(
        pack.get(&root_key)
            .context("resolve checkpoint StateService root node")?
            .is_some(),
        "checkpoint pack does not contain the StateService root node"
    );
    Ok(())
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
    source_tip: SourceTip,
    pack: PackStore,
) -> Result<()> {
    report
        .validate_authoritative(arguments.network_magic)
        .context("validate completed checkpoint metadata")?;
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
    validate_root_node(&reopened, source_tip)?;
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
    use neo_io::SerializableExtensions;
    use tempfile::tempdir;

    fn test_arguments(pack_path: PathBuf, rows_per_frame: usize) -> Arguments {
        Arguments {
            network_magic: 0x334F_454E,
            source: SourceArguments::Mdbx {
                path: PathBuf::new(),
            },
            pack_path,
            rows_per_frame,
            max_rows: None,
            max_index_memory_bytes: 1024 * 1024,
        }
    }

    fn open_test_pack(arguments: &Arguments, source_tip: SourceTip) -> Result<OpenedCheckpoint> {
        open_or_resume_pack(arguments, source_tip, BuildSourceIdentity::Mdbx)
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
    fn incomplete_report_cannot_be_confused_with_a_complete_checkpoint() {
        let report = CheckpointReport {
            schema_version: PACK_CHECKPOINT_SCHEMA_VERSION,
            authoritative_ready: false,
            complete: false,
            source_backend: "mdbx".to_owned(),
            source_namespace: PACK_CHECKPOINT_SOURCE_NAMESPACE.to_owned(),
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
        assert_eq!(json["schema_version"], PACK_CHECKPOINT_SCHEMA_VERSION);
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
        let arguments = test_arguments(pack_path.clone(), 2);
        let source_tip = SourceTip {
            height: 123,
            root_internal: [0x55; 32],
        };
        let opened = open_test_pack(&arguments, source_tip).expect("create partial pack");
        assert_eq!((opened.resumed_rows, opened.frames), (0, 0));
        let mut store = opened.pack;
        store
            .append_frame(checkpoint_frame_context(source_tip), &original)
            .expect("append partial prefix");
        drop(store);

        let opened = open_test_pack(&arguments, source_tip).expect("open partial pack");
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
        let arguments = test_arguments(pack_path.clone(), 1);
        let source_tip = SourceTip {
            height: 123,
            root_internal: [0x55; 32],
        };
        let mut store = open_test_pack(&arguments, source_tip)
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
        let error = open_test_pack(&wrong_network, source_tip)
            .err()
            .expect("network identity drift must reject resume");
        assert!(error.to_string().contains("network magic changed"));

        let changed_tip = SourceTip {
            height: source_tip.height + 1,
            ..source_tip
        };
        let error = open_test_pack(&arguments, changed_tip)
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
        let error = open_test_pack(&arguments, source_tip)
            .err()
            .expect("format identity drift must reject resume");
        assert!(error.to_string().contains("format version changed"));
    }

    #[test]
    fn partial_migration_resume_rejects_a_different_same_tip_stream() {
        let temporary = tempdir().expect("temporary checkpoint parent");
        let pack_path = temporary.path().join("pack");
        let mut arguments = test_arguments(pack_path, 1);
        arguments.source = SourceArguments::MigrationStream {
            stream_path: PathBuf::from("/first-stream"),
            metadata_mdbx_path: PathBuf::from("/metadata"),
        };
        let source_tip = SourceTip {
            height: 123,
            root_internal: [0x55; 32],
        };
        let header = MigrationStreamHeader {
            network_magic: arguments.network_magic,
            height: source_tip.height,
            root_internal: source_tip.root_internal,
            rows: 2,
            value_bytes: 11,
            payload_bytes: 85,
            namespace_sha256: [0x66; 32],
            payload_sha256: [0x77; 32],
        };
        let mut store = open_or_resume_pack(
            &arguments,
            source_tip,
            BuildSourceIdentity::migration_stream(header),
        )
        .expect("create partial migration pack")
        .pack;
        let mut key = [1u8; 33];
        key[0] = STATE_NODE_PREFIX;
        store
            .append_frame(
                checkpoint_frame_context(source_tip),
                &[PackOperation {
                    key,
                    kind: PackOpKind::Put(b"shared-prefix".to_vec()),
                }],
            )
            .expect("append shared stream prefix");
        drop(store);

        let mut different_stream = header;
        different_stream.payload_sha256 = [0x88; 32];
        let error = open_or_resume_pack(
            &arguments,
            source_tip,
            BuildSourceIdentity::migration_stream(different_stream),
        )
        .err()
        .expect("different authenticated stream must reject the partial prefix");
        assert!(error.to_string().contains("source identity"));
    }

    #[test]
    fn migration_build_identity_binds_complete_authenticated_header_geometry() {
        let arguments = test_arguments(PathBuf::from("/checkpoint"), 2);
        let source_tip = SourceTip {
            height: 123,
            root_internal: [0x55; 32],
        };
        let header = MigrationStreamHeader {
            network_magic: arguments.network_magic,
            height: source_tip.height,
            root_internal: source_tip.root_internal,
            rows: 2,
            value_bytes: 11,
            payload_bytes: 85,
            namespace_sha256: [0x66; 32],
            payload_sha256: [0x77; 32],
        };
        let expected = BuildIdentity::current(
            &arguments,
            source_tip,
            BuildSourceIdentity::migration_stream(header),
        );
        let mut candidates = Vec::new();
        candidates.push(MigrationStreamHeader {
            rows: header.rows + 1,
            ..header
        });
        candidates.push(MigrationStreamHeader {
            value_bytes: header.value_bytes + 1,
            ..header
        });
        candidates.push(MigrationStreamHeader {
            payload_bytes: header.payload_bytes + 1,
            ..header
        });
        candidates.push(MigrationStreamHeader {
            namespace_sha256: [0x88; 32],
            ..header
        });
        candidates.push(MigrationStreamHeader {
            payload_sha256: [0x99; 32],
            ..header
        });

        for candidate in candidates {
            let actual = BuildIdentity::current(
                &arguments,
                source_tip,
                BuildSourceIdentity::migration_stream(candidate),
            );
            assert!(validate_build_identity(&actual, &expected).is_err());
        }
    }

    #[test]
    fn mdbx_build_identity_has_no_migration_stream_fields() {
        let arguments = test_arguments(PathBuf::from("/checkpoint"), 2);
        let identity = BuildIdentity::current(
            &arguments,
            SourceTip {
                height: 123,
                root_internal: [0x55; 32],
            },
            BuildSourceIdentity::Mdbx,
        );
        let encoded = serde_json::to_value(identity).expect("encode build identity");
        assert_eq!(encoded["schema_version"], BUILD_IDENTITY_SCHEMA_VERSION);
        assert_eq!(encoded["source"], serde_json::json!({ "backend": "mdbx" }));
        assert!(encoded.get("source_backend").is_none());
    }

    #[test]
    fn partial_pack_resume_accepts_payload_bounded_variable_frame_geometry() {
        let temporary = tempdir().expect("temporary checkpoint parent");
        let pack_path = temporary.path().join("pack");
        let arguments = test_arguments(pack_path.clone(), 2);
        let source_tip = SourceTip {
            height: 123,
            root_internal: [0x55; 32],
        };
        let mut store = open_test_pack(&arguments, source_tip)
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

        let opened = open_test_pack(&arguments, source_tip)
            .expect("variable complete frame geometry is recoverable");
        assert_eq!((opened.resumed_rows, opened.frames), (2, 2));
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
        assert_eq!(
            arguments.source,
            SourceArguments::Mdbx {
                path: PathBuf::from("/source")
            }
        );
        assert_eq!(arguments.pack_path, Path::new("/checkpoint"));

        let error = parse_arguments_from(
            ["--mdbx", "/source", "--pack", "/checkpoint"]
                .into_iter()
                .map(str::to_owned),
        )
        .expect_err("network magic must be explicit");
        assert!(error.to_string().contains("--network-magic is required"));
    }

    #[test]
    fn command_line_requires_exactly_one_complete_source_mode() {
        let migration = parse_arguments_from(
            [
                "--network-magic",
                "0x334F454E",
                "--migration-stream",
                "/stream",
                "--metadata-mdbx",
                "/metadata",
                "--pack",
                "/checkpoint",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .expect("parse migration source");
        assert_eq!(
            migration.source,
            SourceArguments::MigrationStream {
                stream_path: PathBuf::from("/stream"),
                metadata_mdbx_path: PathBuf::from("/metadata"),
            }
        );

        for invalid in [
            vec![
                "--network-magic",
                "0x334F454E",
                "--migration-stream",
                "/stream",
                "--pack",
                "/checkpoint",
            ],
            vec![
                "--network-magic",
                "0x334F454E",
                "--mdbx",
                "/source",
                "--migration-stream",
                "/stream",
                "--metadata-mdbx",
                "/metadata",
                "--pack",
                "/checkpoint",
            ],
            vec![
                "--network-magic",
                "0x334F454E",
                "--migration-stream",
                "/stream",
                "--metadata-mdbx",
                "/metadata",
                "--pack",
                "/checkpoint",
                "--max-rows",
                "1",
            ],
        ] {
            assert!(parse_arguments_from(invalid.into_iter().map(str::to_owned)).is_err());
        }
    }

    #[test]
    fn migration_header_must_match_network_and_coordinated_state_tip() {
        let arguments = Arguments {
            source: SourceArguments::MigrationStream {
                stream_path: PathBuf::from("/stream"),
                metadata_mdbx_path: PathBuf::from("/metadata"),
            },
            ..test_arguments(PathBuf::from("/checkpoint"), 2)
        };
        let source_tip = SourceTip {
            height: 123,
            root_internal: [0x55; 32],
        };
        let header = MigrationStreamHeader {
            network_magic: arguments.network_magic,
            height: source_tip.height,
            root_internal: source_tip.root_internal,
            rows: 2,
            value_bytes: 3,
            payload_bytes: 77,
            namespace_sha256: [0x66; 32],
            payload_sha256: [0x77; 32],
        };
        validate_migration_header(header, &arguments, source_tip).expect("matching identity");
        assert!(
            validate_migration_header(
                MigrationStreamHeader {
                    network_magic: 1,
                    ..header
                },
                &arguments,
                source_tip,
            )
            .is_err()
        );
        assert!(
            validate_migration_header(
                MigrationStreamHeader {
                    height: 124,
                    ..header
                },
                &arguments,
                source_tip,
            )
            .is_err()
        );
    }

    #[test]
    fn shared_accumulator_builds_scrubs_and_reopens_root_reachable_checkpoint() {
        let temporary = tempdir().expect("temporary checkpoint parent");
        let arguments = test_arguments(temporary.path().join("pack"), 1);
        let root_node = Node::new_leaf(b"root-node".to_vec());
        let root_internal = root_node.try_hash().expect("hash root node").to_array();
        let source_tip = SourceTip {
            height: 123,
            root_internal,
        };
        let opened = open_test_pack(&arguments, source_tip).expect("create checkpoint");
        let mut accumulator =
            RowAccumulator::new(&arguments, source_tip, opened).expect("create row accumulator");
        let mut root_key = [0u8; 33];
        root_key[0] = STATE_NODE_PREFIX;
        root_key[1..].copy_from_slice(&source_tip.root_internal);
        let later_node = Node::new_leaf(b"later-node".to_vec());
        let later_hash = later_node.try_hash().expect("hash later node").to_array();
        let mut later_key = [0u8; 33];
        later_key[0] = STATE_NODE_PREFIX;
        later_key[1..].copy_from_slice(&later_hash);
        accumulator
            .push(
                root_key,
                &root_node.to_array().expect("serialize root node"),
            )
            .expect("root row");
        accumulator
            .push(
                later_key,
                &later_node.to_array().expect("serialize later node"),
            )
            .expect("later row");
        let accumulated = accumulator.finish().expect("finish rows");
        assert_eq!((accumulated.rows, accumulated.frames), (2, 2));
        let scrub = accumulated
            .pack
            .scrub_checkpoint_namespace()
            .expect("scrub current pack");
        assert_eq!(scrub.sha256, accumulated.namespace_sha256);
        validate_root_node(&accumulated.pack, source_tip).expect("root reachable");
        drop(accumulated.pack);

        let reopened = PackStore::open(
            &arguments.pack_path,
            pack_store_config(arguments.max_index_memory_bytes).expect("pack config"),
        )
        .expect("reopen current checkpoint");
        validate_root_node(&reopened, source_tip).expect("reopened root reachable");
    }

    #[test]
    fn accumulator_rejects_invalid_mpt_rows_before_accounting() {
        let temporary = tempdir().expect("temporary checkpoint parent");
        let arguments = test_arguments(temporary.path().join("pack"), 1);
        let source_tip = SourceTip {
            height: 123,
            root_internal: [0x55; 32],
        };
        let opened = open_test_pack(&arguments, source_tip).expect("create checkpoint");
        let mut accumulator =
            RowAccumulator::new(&arguments, source_tip, opened).expect("create row accumulator");
        let mut key = [0u8; 33];
        key[0] = STATE_NODE_PREFIX;
        key[1..].copy_from_slice(&source_tip.root_internal);

        let error = accumulator
            .push(key, b"not-a-persisted-mpt-node")
            .expect_err("malformed MPT rows must fail before entering the pack");
        assert!(
            error
                .to_string()
                .contains("validate checkpoint MPT node row")
        );
        assert_eq!(accumulator.rows, 0);
        assert_eq!(accumulator.value_bytes, 0);
        assert!(accumulator.operations.is_empty());
    }
}
