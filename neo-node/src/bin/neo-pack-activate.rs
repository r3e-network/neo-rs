//! Atomically activate one fully verified authoritative StateService checkpoint.
//!
//! This offline migration tool never decodes the preceding marker. Its exact
//! digest and bytes are compare-exchanged together with the canonical Ledger
//! tip and both StateService root rows in one MDBX write transaction.
//!
//! Usage:
//!   neo-pack-activate --network-magic <u32-or-hex>
//!     --mdbx <canonical-store-dir> --pack <current-pack-dir>
//!     --expected-marker-sha256 <hex> --report <new-json-path> --activate
//!     [--max-index-memory-mb N] [--max-root-nodes N]
//!     [--max-root-bytes-gb N]

mod neo_pack_common;

use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use anyhow::{Context, Result, bail, ensure};
use neo_crypto::Crypto;
use neo_native_contracts::LedgerContract;
use neo_node::NodeLifecycleLock;
use neo_primitives::UInt256;
use neo_state_packs::authority::{AUTHORITATIVE_HIGH_WATER_KEY, AuthoritativeHighWaterRecord};
use neo_state_packs::checkpoint::PackCheckpoint;
use neo_state_packs::{PACK_KEY_BYTES, PackStore, PackStoreConfig};
use neo_state_service::{Keys, MDBX_STATE_SERVICE_NAMESPACE, read_current_local_root_from};
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::storage::StorageConfig;
use neo_storage::persistence::{
    CoordinatedCommitMarker, RawReadOnlyStore, Store, StoreCache, StoreFactory, StoreValueGuard,
    TransactionalStore,
};
use neo_trie::{MPT_NODE_PREFIX, Node, PersistedMptGraphLimits};
use serde::{Deserialize, Serialize};

use neo_pack_common::{
    DEFAULT_MAX_ROOT_GRAPH_BYTES, DEFAULT_MAX_ROOT_GRAPH_NODES, MAX_MPT_NODE_BYTES,
    validate_pack_root_graph,
};

const DEFAULT_MAX_INDEX_MEMORY_MB: u64 = 512;
const MAX_ACTIVATION_REPORT_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Arguments {
    network_magic: u32,
    mdbx_path: PathBuf,
    pack_path: PathBuf,
    expected_marker_sha256: [u8; 32],
    report_path: PathBuf,
    max_index_memory_bytes: u64,
    max_root_nodes: u64,
    max_root_bytes: u64,
}

#[derive(Debug)]
struct GuardedTip {
    height: u32,
    root_internal: [u8; 32],
    ledger: StoreValueGuard,
    state_service: [StoreValueGuard; 2],
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ActivationReport {
    schema_version: u32,
    network_magic: String,
    checkpoint_path: String,
    checkpoint_identity_sha256: String,
    checkpoint_rows: u64,
    checkpoint_value_bytes: u64,
    block_index: u32,
    state_root: String,
    state_root_internal_bytes: String,
    tip_epoch: u64,
    tip_segment_id: u64,
    tip_frame_end: u64,
    tip_frame_sha256: String,
    scrubbed_frames: u64,
    scrubbed_rows: u64,
    scrubbed_value_bytes: u64,
    scrubbed_index_runs: u64,
    scrubbed_index_records: u64,
    bound_index_live_runs: u64,
    bound_index_source_records: u64,
    bound_index_records_sha256: String,
    root_graph_max_nodes: u64,
    root_graph_max_total_bytes: u64,
    root_graph_max_node_bytes: u64,
    root_graph_unique_nodes: u64,
    root_graph_total_bytes: u64,
    root_graph_branch_nodes: u64,
    root_graph_extension_nodes: u64,
    root_graph_leaf_nodes: u64,
    preceding_marker_sha256: String,
    activated_marker_sha256: String,
    preflight_elapsed_seconds: f64,
}

#[derive(Debug)]
enum MarkerState {
    Legacy(Vec<u8>),
    Activated,
}

#[derive(Debug)]
struct ReportFiles {
    published: bool,
    prepared: bool,
    report: Option<ActivationReport>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActivationBoundary {
    ReportPrepared,
    MarkerCommitted,
}

fn main() -> Result<()> {
    let arguments = parse_arguments()?;
    activate(&arguments)
}

fn activate(arguments: &Arguments) -> Result<()> {
    activate_with_hook(arguments, |_| Ok(()))
}

fn activate_with_hook(
    arguments: &Arguments,
    mut reached: impl FnMut(ActivationBoundary) -> Result<()>,
) -> Result<()> {
    let started = Instant::now();
    let lifecycle_lock = NodeLifecycleLock::acquire(&arguments.mdbx_path)
        .context("exclude the running node from checkpoint activation")?;
    eprintln!(
        "acquired canonical node-data lifecycle lock at {}",
        lifecycle_lock.path().display()
    );
    let report_temporary = temporary_report_path(&arguments.report_path)?;
    let report_staging = staging_report_path(&report_temporary)?;

    let checkpoint = PackCheckpoint::read(&arguments.pack_path)
        .context("read strict current-format checkpoint")?;
    let validated = checkpoint
        .validate_authoritative(arguments.network_magic)
        .context("validate authoritative checkpoint binding")?;
    let horizon = validated.commit_horizon();
    let pack_config = PackStoreConfig::default()
        .with_max_index_memory_bytes(arguments.max_index_memory_bytes)
        .context("validate pack index-memory bound")?;

    let canonical: Arc<RuntimeStore> = StoreFactory::get_store_with_config(
        "mdbx",
        StorageConfig {
            path: arguments.mdbx_path.clone(),
            read_only: false,
            ..StorageConfig::default()
        },
    )
    .map_err(|error| anyhow::anyhow!("open writable MDBX store: {error}"))?;
    let state_store = canonical
        .open_coordinated_namespace(MDBX_STATE_SERVICE_NAMESPACE)
        .context("open coordinated MDBX StateService namespace")?;
    let guarded_tip = read_guarded_tip(&canonical, &state_store)?;
    ensure!(
        guarded_tip.height == checkpoint.source_height
            && guarded_tip.root_internal == validated.source_root_internal(),
        "Ledger/StateService tip differs from the checkpoint source tip"
    );
    let marker = AuthoritativeHighWaterRecord {
        network_magic: arguments.network_magic,
        store_identity: validated.store_identity(),
        epoch: horizon.epoch,
        segment_id: horizon.segment_id,
        frame_end: horizon.frame_end,
        frame_sha256: horizon.frame_sha256,
        frame_context: horizon.context,
        block_index: guarded_tip.height,
        state_root: guarded_tip.root_internal,
    };
    marker
        .validate_identity(arguments.network_magic, validated.store_identity())
        .context("validate replacement marker identity")?;
    let encoded_marker = marker.encode();
    let activated_marker_sha256 = Crypto::sha256(&encoded_marker);
    let persisted_marker = state_store
        .maintenance_metadata(AUTHORITATIVE_HIGH_WATER_KEY)
        .context("read authoritative marker")?
        .context("authoritative marker is absent")?;
    let marker_state = classify_marker(
        persisted_marker,
        arguments.expected_marker_sha256,
        &encoded_marker,
    )?;
    recover_staging_report(&report_staging, &report_temporary, &marker_state)?;

    let pack = PackStore::open_at_commit_horizon(&arguments.pack_path, pack_config, Some(horizon))
        .with_context(|| {
            format!(
                "open authoritative checkpoint at {}",
                arguments.pack_path.display()
            )
        })?;
    let receipt = pack
        .last_frame_receipt()
        .context("authoritative checkpoint has no pack tip")?;
    ensure!(
        receipt.epoch == horizon.epoch
            && receipt.segment_id == horizon.segment_id
            && receipt.frame_end == horizon.frame_end
            && receipt.context == horizon.context
            && receipt.frame_sha256 == horizon.frame_sha256,
        "checkpoint pack tip differs from checkpoint.json"
    );
    ensure!(
        marker
            == AuthoritativeHighWaterRecord::new(
                arguments.network_magic,
                validated.store_identity(),
                receipt,
                guarded_tip.height,
                guarded_tip.root_internal,
            ),
        "checkpoint frame receipt differs from the preflight marker horizon"
    );
    let opened = pack.open_validation();
    ensure!(
        opened.frames == checkpoint.frames && opened.index_entries == checkpoint.rows,
        "checkpoint pack geometry differs from checkpoint.json"
    );
    let checkpoint_evidence = pack
        .checkpoint_evidence()
        .context("bind the complete checkpoint payload and materialized indexes")?;
    let payload = checkpoint_evidence.namespace;
    ensure!(
        payload.sha256 == validated.store_identity(),
        "checkpoint namespace digest differs from checkpoint.json"
    );
    ensure!(
        payload.scrub.frames == checkpoint.scrubbed_frames
            && payload.scrub.rows == checkpoint.scrubbed_rows
            && payload.scrub.puts == checkpoint.scrubbed_puts
            && payload.scrub.tombstones == checkpoint.scrubbed_tombstones
            && payload.scrub.payload_bytes == checkpoint.scrubbed_payload_bytes
            && payload.scrub.value_bytes == checkpoint.scrubbed_value_bytes,
        "checkpoint payload scrub geometry differs from checkpoint.json"
    );
    let index_scrub = pack
        .scrub_index_runs()
        .context("scrub every checkpoint index run")?;
    ensure!(
        index_scrub.records == checkpoint.rows,
        "checkpoint index record count differs from checkpoint rows"
    );
    let index_binding = checkpoint_evidence.index;
    ensure!(
        index_binding.frame_records == checkpoint.rows
            && index_binding.winner_records == checkpoint.rows
            && index_binding.value_bytes == checkpoint.value_bytes,
        "checkpoint index-binding evidence differs from checkpoint.json"
    );
    let root_graph_limits = root_graph_limits(arguments, &checkpoint)?;
    eprintln!(
        "checkpoint root-graph audit: max_nodes={} max_total_bytes={} max_node_bytes={}",
        root_graph_limits.max_nodes,
        root_graph_limits.max_total_bytes,
        root_graph_limits.max_node_bytes
    );
    let root_graph =
        validate_pack_root_graph(&pack, validated.source_root_internal(), root_graph_limits)?;

    let expected_report = ActivationReport {
        schema_version: 1,
        network_magic: format!("0x{:08X}", arguments.network_magic),
        checkpoint_path: arguments.pack_path.display().to_string(),
        checkpoint_identity_sha256: formatted_hash(validated.store_identity()),
        checkpoint_rows: checkpoint.rows,
        checkpoint_value_bytes: checkpoint.value_bytes,
        block_index: guarded_tip.height,
        state_root: displayed_root(guarded_tip.root_internal),
        state_root_internal_bytes: formatted_hash(guarded_tip.root_internal),
        tip_epoch: receipt.epoch,
        tip_segment_id: receipt.segment_id.get(),
        tip_frame_end: receipt.frame_end,
        tip_frame_sha256: formatted_hash(receipt.frame_sha256),
        scrubbed_frames: payload.scrub.frames,
        scrubbed_rows: payload.scrub.rows,
        scrubbed_value_bytes: payload.scrub.value_bytes,
        scrubbed_index_runs: index_scrub.runs,
        scrubbed_index_records: index_scrub.records,
        bound_index_live_runs: index_binding.live_runs,
        bound_index_source_records: index_binding.source_records,
        bound_index_records_sha256: formatted_hash(index_binding.records_sha256),
        root_graph_max_nodes: root_graph_limits.max_nodes,
        root_graph_max_total_bytes: root_graph_limits.max_total_bytes,
        root_graph_max_node_bytes: root_graph_limits.max_node_bytes,
        root_graph_unique_nodes: root_graph.unique_nodes,
        root_graph_total_bytes: root_graph.total_bytes,
        root_graph_branch_nodes: root_graph.branch_nodes,
        root_graph_extension_nodes: root_graph.extension_nodes,
        root_graph_leaf_nodes: root_graph.leaf_nodes,
        preceding_marker_sha256: formatted_hash(arguments.expected_marker_sha256),
        activated_marker_sha256: formatted_hash(activated_marker_sha256),
        preflight_elapsed_seconds: started.elapsed().as_secs_f64(),
    };
    let mut reports =
        inspect_report_files(&arguments.report_path, &report_temporary, &expected_report)?;
    match &marker_state {
        MarkerState::Legacy(_) => {
            ensure!(
                !reports.published,
                "activation report is published while the legacy marker remains"
            );
            if !reports.prepared {
                write_prepared_report(&report_staging, &report_temporary, &expected_report)?;
                reports.prepared = true;
                reports.report = Some(expected_report.clone());
            }
            // A prepared name recovered after a crash may be visible without
            // having crossed its original directory fence. Re-fence both the
            // report contents and its name before the marker can commit.
            sync_report_file(&report_temporary)?;
        }
        MarkerState::Activated => ensure!(
            reports.report.is_some(),
            "target marker is already active but no durable activation report can prove the preceding marker"
        ),
    }
    let report = reports
        .report
        .clone()
        .context("activation report state is incomplete")?;
    reached(ActivationBoundary::ReportPrepared)?;

    if let MarkerState::Legacy(preceding_marker) = marker_state {
        canonical
            .compare_exchange_coordinated_marker(
                &state_store,
                std::slice::from_ref(&guarded_tip.ledger),
                &guarded_tip.state_service,
                &StoreValueGuard::present(AUTHORITATIVE_HIGH_WATER_KEY.to_vec(), preceding_marker),
                &CoordinatedCommitMarker {
                    key: AUTHORITATIVE_HIGH_WATER_KEY.to_vec(),
                    value: encoded_marker.to_vec(),
                },
            )
            .context("atomically activate checkpoint marker")?;
        reached(ActivationBoundary::MarkerCommitted)?;
    }

    verify_published_marker(
        &state_store,
        arguments.network_magic,
        validated.store_identity(),
        marker,
    )?;
    ensure_root_node(&pack, validated.source_root_internal())?;
    drop(pack);
    let reopened = PackStore::open_at_commit_horizon(
        &arguments.pack_path,
        pack_config,
        Some(marker.commit_horizon()),
    )
    .context("reopen checkpoint at the activated marker")?;
    ensure!(
        reopened.last_frame_receipt() == Some(receipt),
        "reopened checkpoint tip differs after marker activation"
    );
    ensure_root_node(&reopened, validated.source_root_internal())?;

    if reports.published {
        if reports.prepared {
            remove_prepared_report(&report_temporary, &arguments.report_path)?;
        }
    } else {
        ensure!(
            reports.prepared,
            "activation report was not prepared before publication"
        );
        publish_report(&report_temporary, &arguments.report_path)?;
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn read_guarded_tip(
    canonical: &Arc<RuntimeStore>,
    state_store: &RuntimeStore,
) -> Result<GuardedTip> {
    let ledger_snapshot = canonical.snapshot();
    let ledger_key = LedgerContract::current_block_storage_key().to_array();
    let ledger_value = required_raw(ledger_snapshot.as_ref(), &ledger_key, "Ledger current tip")?;
    let ledger_cache = StoreCache::<RuntimeStore>::new_from_snapshot(Arc::clone(&ledger_snapshot));
    let (_, ledger_height) = LedgerContract::new()
        .optional_current_tip(ledger_cache.data_cache())
        .context("decode Ledger current tip")?
        .context("Ledger current tip is absent")?;

    let state_snapshot = state_store.snapshot();
    let current_root_key = Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec();
    let current_root_value = required_raw(
        state_snapshot.as_ref(),
        &current_root_key,
        "StateService current-local-root pointer",
    )?;
    let state_root = read_current_local_root_from(state_snapshot.as_ref())
        .context("decode current local StateService root")?;
    ensure!(
        state_root.index() == ledger_height,
        "Ledger tip height differs from StateService tip height"
    );
    let root_key = Keys::state_root(state_root.index());
    let root_value = required_raw(
        state_snapshot.as_ref(),
        &root_key,
        "StateService current root record",
    )?;
    Ok(GuardedTip {
        height: ledger_height,
        root_internal: state_root.root_hash().to_array(),
        ledger: StoreValueGuard::present(ledger_key, ledger_value),
        state_service: [
            StoreValueGuard::present(current_root_key, current_root_value),
            StoreValueGuard::present(root_key, root_value),
        ],
    })
}

fn required_raw<R>(store: &R, key: &[u8], name: &str) -> Result<Vec<u8>>
where
    R: RawReadOnlyStore + ?Sized,
{
    store
        .try_get_bytes_result(key)
        .with_context(|| format!("read {name}"))?
        .with_context(|| format!("{name} is absent"))
}

fn ensure_root_node(pack: &PackStore, root: [u8; 32]) -> Result<()> {
    let mut key = [0u8; PACK_KEY_BYTES];
    key[0] = MPT_NODE_PREFIX;
    key[1..].copy_from_slice(&root);
    let value = pack
        .get_bounded(&key, MAX_MPT_NODE_BYTES)
        .context("resolve checkpoint StateService root node")?
        .context("checkpoint does not contain its StateService root node")?;
    let expected_hash = UInt256::from_bytes(&root).context("decode checkpoint StateRoot bytes")?;
    Node::validate_persisted(&value, expected_hash)
        .context("validate checkpoint StateService root node")?;
    Ok(())
}

fn root_graph_limits(
    arguments: &Arguments,
    checkpoint: &PackCheckpoint,
) -> Result<PersistedMptGraphLimits> {
    ensure!(
        checkpoint.rows > 0 && checkpoint.value_bytes > 0,
        "checkpoint MPT namespace is empty"
    );
    let max_nodes = arguments.max_root_nodes.min(checkpoint.rows);
    let max_total_bytes = arguments.max_root_bytes.min(checkpoint.value_bytes);
    Ok(PersistedMptGraphLimits::new(
        max_nodes,
        max_total_bytes,
        MAX_MPT_NODE_BYTES.min(max_total_bytes),
    ))
}

fn verify_published_marker(
    state_store: &RuntimeStore,
    network_magic: u32,
    store_identity: [u8; 32],
    expected: AuthoritativeHighWaterRecord,
) -> Result<()> {
    let bytes = state_store
        .maintenance_metadata(AUTHORITATIVE_HIGH_WATER_KEY)
        .context("read activated marker")?
        .context("activated marker is absent")?;
    let actual = AuthoritativeHighWaterRecord::decode(&bytes)
        .context("decode activated current-format marker")?;
    actual
        .validate_identity(network_magic, store_identity)
        .context("validate activated marker identity")?;
    ensure!(actual == expected, "activated marker differs after commit");
    Ok(())
}

fn parse_arguments() -> Result<Arguments> {
    parse_arguments_from(std::env::args().skip(1))
}

fn parse_arguments_from(arguments: impl IntoIterator<Item = String>) -> Result<Arguments> {
    let mut network_magic = None;
    let mut mdbx_path = None;
    let mut pack_path = None;
    let mut expected_marker_sha256 = None;
    let mut report_path = None;
    let mut max_index_memory_mb = DEFAULT_MAX_INDEX_MEMORY_MB;
    let mut max_root_nodes = DEFAULT_MAX_ROOT_GRAPH_NODES;
    let mut max_root_bytes_gb = DEFAULT_MAX_ROOT_GRAPH_BYTES / (1024 * 1024 * 1024);
    let mut activate = false;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--network-magic" => {
                network_magic = Some(parse_u32_literal(&required_argument(
                    &mut arguments,
                    "--network-magic",
                )?)?);
            }
            "--mdbx" => {
                mdbx_path = Some(PathBuf::from(required_argument(&mut arguments, "--mdbx")?))
            }
            "--pack" => {
                pack_path = Some(PathBuf::from(required_argument(&mut arguments, "--pack")?))
            }
            "--expected-marker-sha256" => {
                expected_marker_sha256 = Some(parse_hash(&required_argument(
                    &mut arguments,
                    "--expected-marker-sha256",
                )?)?);
            }
            "--report" => {
                report_path = Some(PathBuf::from(required_argument(
                    &mut arguments,
                    "--report",
                )?));
            }
            "--max-index-memory-mb" => {
                max_index_memory_mb = required_argument(&mut arguments, "--max-index-memory-mb")?
                    .parse::<u64>()
                    .context("--max-index-memory-mb requires a positive integer")?;
                ensure!(
                    max_index_memory_mb > 0,
                    "--max-index-memory-mb must be positive"
                );
            }
            "--max-root-nodes" => {
                max_root_nodes = required_argument(&mut arguments, "--max-root-nodes")?
                    .parse::<u64>()
                    .context("--max-root-nodes requires a positive integer")?;
                ensure!(max_root_nodes > 0, "--max-root-nodes must be positive");
            }
            "--max-root-bytes-gb" => {
                max_root_bytes_gb = required_argument(&mut arguments, "--max-root-bytes-gb")?
                    .parse::<u64>()
                    .context("--max-root-bytes-gb requires a positive integer")?;
                ensure!(
                    max_root_bytes_gb > 0,
                    "--max-root-bytes-gb must be positive"
                );
            }
            "--activate" => activate = true,
            other => bail!("unknown argument {other}"),
        }
    }
    ensure!(
        activate,
        "--activate is required for the mutating operation"
    );
    let max_index_memory_bytes = max_index_memory_mb
        .checked_mul(1024 * 1024)
        .context("--max-index-memory-mb overflows bytes")?;
    let max_root_bytes = max_root_bytes_gb
        .checked_mul(1024 * 1024 * 1024)
        .context("--max-root-bytes-gb overflows bytes")?;
    Ok(Arguments {
        network_magic: network_magic.context("--network-magic is required")?,
        mdbx_path: mdbx_path.context("--mdbx is required")?,
        pack_path: pack_path.context("--pack is required")?,
        expected_marker_sha256: expected_marker_sha256
            .context("--expected-marker-sha256 is required")?,
        report_path: report_path.context("--report is required")?,
        max_index_memory_bytes,
        max_root_nodes,
        max_root_bytes,
    })
}

fn required_argument(arguments: &mut impl Iterator<Item = String>, name: &str) -> Result<String> {
    arguments
        .next()
        .with_context(|| format!("{name} requires a value"))
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

fn parse_hash(value: &str) -> Result<[u8; 32]> {
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    let bytes = hex::decode(value).context("SHA-256 must be hexadecimal")?;
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| anyhow::anyhow!("SHA-256 is {} bytes, expected 32", bytes.len()))
}

fn temporary_report_path(report: &Path) -> Result<PathBuf> {
    let name = report
        .file_name()
        .context("--report must name a file")?
        .to_string_lossy();
    Ok(report.with_file_name(format!("{name}.tmp")))
}

fn staging_report_path(temporary: &Path) -> Result<PathBuf> {
    let name = temporary
        .file_name()
        .context("temporary report path must name a file")?
        .to_string_lossy();
    Ok(temporary.with_file_name(format!("{name}.staging")))
}

fn report_directory(path: &Path) -> Result<&Path> {
    let parent = path
        .parent()
        .context("activation report must name a file")?;
    if parent.as_os_str().is_empty() {
        Ok(Path::new("."))
    } else {
        Ok(parent)
    }
}

fn classify_marker(
    persisted: Vec<u8>,
    expected_legacy_sha256: [u8; 32],
    expected_activated: &[u8],
) -> Result<MarkerState> {
    if persisted == expected_activated {
        return Ok(MarkerState::Activated);
    }
    ensure!(
        AuthoritativeHighWaterRecord::decode(&persisted).is_err(),
        "a different current-format authoritative marker is already active"
    );
    ensure!(
        Crypto::sha256(&persisted) == expected_legacy_sha256,
        "preceding marker SHA-256 differs from --expected-marker-sha256"
    );
    Ok(MarkerState::Legacy(persisted))
}

fn inspect_report_files(
    report: &Path,
    temporary: &Path,
    expected: &ActivationReport,
) -> Result<ReportFiles> {
    let parent = report_directory(report)?;
    ensure!(parent.is_dir(), "report parent directory does not exist");
    let published = read_existing_report(report, expected)?;
    let prepared = read_existing_report(temporary, expected)?;
    if let (Some(published), Some(prepared)) = (&published, &prepared) {
        ensure!(
            published == prepared,
            "published and prepared activation reports differ"
        );
    }
    Ok(ReportFiles {
        published: published.is_some(),
        prepared: prepared.is_some(),
        report: published.or(prepared),
    })
}

fn recover_staging_report(staging: &Path, prepared: &Path, marker: &MarkerState) -> Result<()> {
    let metadata = match fs::symlink_metadata(staging) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect staging report {}", staging.display()));
        }
    };
    ensure!(
        metadata.file_type().is_file(),
        "staging activation report {} is not a regular file",
        staging.display()
    );
    ensure!(
        matches!(marker, MarkerState::Legacy(_)),
        "staging activation report remains after the target marker was activated"
    );
    let parent = report_directory(staging)?;
    ensure!(
        report_directory(prepared)? == parent,
        "prepared and staging reports must share one directory"
    );

    if prepared.try_exists().with_context(|| {
        format!(
            "inspect prepared activation report {} during staging recovery",
            prepared.display()
        )
    })? {
        // If the staging hard link survived, first make the prepared name
        // durable. Only then is it safe to discard the staging name.
        sync_directory(parent)?;
    }
    fs::remove_file(staging)
        .with_context(|| format!("remove incomplete staging report {}", staging.display()))?;
    sync_directory(parent)
}

fn read_existing_report(
    path: &Path,
    expected: &ActivationReport,
) -> Result<Option<ActivationReport>> {
    let file = match open_regular_report(path)? {
        Some(file) => file,
        None => return Ok(None),
    };
    let bytes = file
        .metadata()
        .with_context(|| format!("stat activation report {}", path.display()))?
        .len();
    ensure!(
        bytes > 0 && bytes <= MAX_ACTIVATION_REPORT_BYTES,
        "activation report {} is {bytes} bytes, expected 1..={MAX_ACTIVATION_REPORT_BYTES}",
        path.display()
    );
    let actual: ActivationReport = serde_json::from_reader(BufReader::new(file))
        .with_context(|| format!("decode activation report {}", path.display()))?;
    ensure!(
        actual.preflight_elapsed_seconds.is_finite()
            && actual.preflight_elapsed_seconds.is_sign_positive(),
        "activation report {} has an invalid preflight duration",
        path.display()
    );
    let mut comparable = expected.clone();
    comparable.preflight_elapsed_seconds = actual.preflight_elapsed_seconds;
    ensure!(
        actual == comparable,
        "activation report {} does not bind the requested checkpoint transition",
        path.display()
    );
    Ok(Some(actual))
}

fn write_prepared_report(staging: &Path, prepared: &Path, report: &ActivationReport) -> Result<()> {
    let parent = report_directory(prepared)?;
    ensure!(
        report_directory(staging)? == parent,
        "prepared and staging reports must share one directory"
    );
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(staging)
        .with_context(|| format!("create staging activation report {}", staging.display()))?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, report).context("encode activation report")?;
    writer
        .write_all(b"\n")
        .context("finish activation report")?;
    writer.flush().context("flush activation report")?;
    writer
        .get_ref()
        .sync_all()
        .context("sync staging activation report")?;

    fs::hard_link(staging, prepared).with_context(|| {
        format!(
            "promote staging activation report {} as {}",
            staging.display(),
            prepared.display()
        )
    })?;
    sync_directory(parent)?;
    fs::remove_file(staging)
        .with_context(|| format!("remove staging report link {}", staging.display()))?;
    sync_directory(parent)
}

fn publish_report(temporary: &Path, report: &Path) -> Result<()> {
    let parent = report_directory(report)?;
    ensure!(
        report_directory(temporary)? == parent,
        "published and prepared reports must share one directory"
    );
    fs::hard_link(temporary, report).with_context(|| {
        format!(
            "publish prepared activation report {} as {}",
            temporary.display(),
            report.display()
        )
    })?;
    // Persist the published name while the prepared name still exists. A
    // crash after this fence can never lose both names.
    sync_report_file(report)?;
    fs::remove_file(temporary)
        .with_context(|| format!("remove prepared report link {}", temporary.display()))?;
    sync_directory(parent)
}

fn remove_prepared_report(temporary: &Path, report: &Path) -> Result<()> {
    let parent = report_directory(report)?;
    ensure!(
        report_directory(temporary)? == parent,
        "published and prepared reports must share one directory"
    );
    // A recovered final link may have been created immediately before a
    // crash. Fence it before removing the already durable prepared link.
    sync_report_file(report).context("published activation report disappeared before recovery")?;
    fs::remove_file(temporary)
        .with_context(|| format!("remove prepared report link {}", temporary.display()))?;
    sync_directory(parent)
}

fn open_regular_report(path: &Path) -> Result<Option<File>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);

    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("open activation report {}", path.display()));
        }
    };
    let metadata = file
        .metadata()
        .with_context(|| format!("stat activation report {}", path.display()))?;
    ensure!(
        metadata.is_file(),
        "activation report {} is not a regular file",
        path.display()
    );
    Ok(Some(file))
}

fn sync_report_file(path: &Path) -> Result<()> {
    open_regular_report(path)?
        .with_context(|| format!("activation report {} is absent", path.display()))?
        .sync_all()
        .with_context(|| format!("sync activation report {}", path.display()))?;
    sync_directory(report_directory(path)?)
}

fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)
        .with_context(|| format!("open directory {} for sync", path.display()))?
        .sync_all()
        .with_context(|| format!("sync directory {}", path.display()))
}

fn formatted_hash(hash: [u8; 32]) -> String {
    format!("0x{}", hex::encode(hash))
}

fn displayed_root(mut internal: [u8; 32]) -> String {
    internal.reverse();
    formatted_hash(internal)
}

#[cfg(test)]
#[path = "neo_pack_activate/tests.rs"]
mod tests;
