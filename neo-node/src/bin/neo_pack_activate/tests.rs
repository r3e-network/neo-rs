use super::*;
use neo_io::SerializableExtensions;
use neo_state_packs::checkpoint::{
    PACK_CHECKPOINT_FILE, PACK_CHECKPOINT_SCHEMA_VERSION, PACK_CHECKPOINT_SOURCE_NAMESPACE,
};
use neo_state_packs::{
    PACK_FRAME_FORMAT_VERSION, PACK_INDEX_FORMAT_VERSION, PACK_MANIFEST_FORMAT_VERSION,
    PACK_SEGMENT_FORMAT_VERSION, PackFrameContext, PackFrameReceipt, PackOpKind, PackOperation,
    PackSegmentId,
};
use neo_state_service::StateRoot;
use neo_storage::persistence::StoreMaintenanceBatch;

fn valid_arguments() -> Vec<String> {
    vec![
        "--network-magic".to_owned(),
        "0x334F454E".to_owned(),
        "--mdbx".to_owned(),
        "db".to_owned(),
        "--pack".to_owned(),
        "pack".to_owned(),
        "--expected-marker-sha256".to_owned(),
        "11".repeat(32),
        "--report".to_owned(),
        "activation.json".to_owned(),
        "--max-index-memory-mb".to_owned(),
        "64".to_owned(),
        "--max-root-nodes".to_owned(),
        "1234".to_owned(),
        "--max-root-bytes-gb".to_owned(),
        "8".to_owned(),
        "--activate".to_owned(),
    ]
}

fn sample_report() -> ActivationReport {
    ActivationReport {
        schema_version: 1,
        network_magic: "0x334F454E".to_owned(),
        checkpoint_path: "/checkpoint".to_owned(),
        checkpoint_identity_sha256: formatted_hash([1; 32]),
        checkpoint_rows: 10,
        checkpoint_value_bytes: 20,
        block_index: 30,
        state_root: displayed_root([2; 32]),
        state_root_internal_bytes: formatted_hash([2; 32]),
        tip_epoch: 3,
        tip_segment_id: 4,
        tip_frame_end: 5,
        tip_frame_sha256: formatted_hash([6; 32]),
        scrubbed_frames: 7,
        scrubbed_rows: 10,
        scrubbed_value_bytes: 20,
        scrubbed_index_runs: 8,
        scrubbed_index_records: 10,
        bound_index_live_runs: 8,
        bound_index_source_records: 10,
        bound_index_records_sha256: formatted_hash([9; 32]),
        root_graph_max_nodes: 10,
        root_graph_max_total_bytes: 20,
        root_graph_max_node_bytes: 11,
        root_graph_unique_nodes: 12,
        root_graph_total_bytes: 13,
        root_graph_branch_nodes: 14,
        root_graph_extension_nodes: 15,
        root_graph_leaf_nodes: 16,
        preceding_marker_sha256: formatted_hash([17; 32]),
        activated_marker_sha256: formatted_hash([18; 32]),
        preflight_elapsed_seconds: 1.25,
    }
}

fn current_marker(identity_byte: u8) -> Vec<u8> {
    let root = [0x22; 32];
    AuthoritativeHighWaterRecord::new(
        0x334F454E,
        [identity_byte; 32],
        PackFrameReceipt {
            epoch: 2,
            segment_id: PackSegmentId::new(1),
            frame_start: 3,
            frame_end: 4_096,
            context: PackFrameContext::new(5, 5, root, root),
            rows: 6,
            metadata_bytes: 7,
            value_bytes: 8,
            frame_sha256: [9; 32],
        },
        5,
        root,
    )
    .encode()
    .to_vec()
}

fn activation_fixture(root: &Path) -> Arguments {
    let network_magic = 0x334F454E;
    let height = 123;
    let mdbx_path = root.join("ledger");
    let pack_path = root.join("pack");
    let report_path = root.join("activation.json");
    let legacy_marker = b"legacy-authoritative-marker".to_vec();

    let node = Node::new_leaf(b"root-value".to_vec());
    let state_root = node.try_hash().expect("hash root node");
    let root_internal = state_root.to_array();
    let node_bytes = node.to_array().expect("serialize root node");
    let mut node_key = [0u8; PACK_KEY_BYTES];
    node_key[0] = MPT_NODE_PREFIX;
    node_key[1..].copy_from_slice(&root_internal);

    let pack_config = PackStoreConfig::default()
        .with_max_index_memory_bytes(16 * 1024 * 1024)
        .expect("test pack config");
    let mut pack = PackStore::create(&pack_path, pack_config).expect("create checkpoint pack");
    pack.append_frame(
        PackFrameContext::new(height, height, root_internal, root_internal),
        &[PackOperation {
            key: node_key,
            kind: PackOpKind::Put(node_bytes.clone()),
        }],
    )
    .expect("append root node");
    let receipt = pack
        .last_frame_receipt()
        .expect("checkpoint pack has a tip");
    let evidence = pack.checkpoint_evidence().expect("checkpoint evidence");
    let (pack_bytes, live_index_bytes, live_runs, decoded_index_memory_bytes) =
        pack.layout().expect("checkpoint layout");
    let checkpoint = PackCheckpoint {
        schema_version: PACK_CHECKPOINT_SCHEMA_VERSION,
        authoritative_ready: true,
        complete: true,
        source_backend: "mdbx".to_owned(),
        source_namespace: PACK_CHECKPOINT_SOURCE_NAMESPACE.to_owned(),
        network_magic: format!("0x{network_magic:08X}"),
        source_height: height,
        source_root: displayed_root(root_internal),
        source_root_internal_bytes: formatted_hash(root_internal),
        source_namespace_sha256: formatted_hash(evidence.namespace.sha256),
        rows: 1,
        resumed_rows: 0,
        value_bytes: node_bytes.len() as u64,
        frames: 1,
        rows_per_frame: 1,
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
        tip_epoch: receipt.epoch,
        tip_segment_id: receipt.segment_id.get(),
        tip_frame_end: receipt.frame_end,
        tip_frame_sha256: formatted_hash(receipt.frame_sha256),
        scrubbed_frames: evidence.namespace.scrub.frames,
        scrubbed_rows: evidence.namespace.scrub.rows,
        scrubbed_puts: evidence.namespace.scrub.puts,
        scrubbed_tombstones: evidence.namespace.scrub.tombstones,
        scrubbed_payload_bytes: evidence.namespace.scrub.payload_bytes,
        scrubbed_value_bytes: evidence.namespace.scrub.value_bytes,
        scrub_elapsed_seconds: 0.001,
        elapsed_seconds: 0.002,
    };
    fs::write(
        pack_path.join(PACK_CHECKPOINT_FILE),
        serde_json::to_vec_pretty(&checkpoint).expect("encode checkpoint"),
    )
    .expect("write checkpoint");
    drop(pack);

    let canonical: Arc<RuntimeStore> = StoreFactory::get_store_with_config(
        "mdbx",
        StorageConfig {
            path: mdbx_path.clone(),
            ..StorageConfig::default()
        },
    )
    .expect("open fixture MDBX");
    let ledger = LedgerContract::new();
    let mut ledger_batch = StoreMaintenanceBatch::new();
    ledger_batch.put_data(
        LedgerContract::current_block_storage_key().to_array(),
        ledger
            .serialize_hash_index_state(&UInt256::from([0x44; 32]), height)
            .expect("serialize ledger tip"),
    );
    canonical
        .commit_maintenance(&ledger_batch)
        .expect("commit ledger tip");

    let state_store = canonical
        .open_coordinated_namespace(MDBX_STATE_SERVICE_NAMESPACE)
        .expect("open fixture StateService namespace");
    let mut state_batch = StoreMaintenanceBatch::new();
    state_batch.put_data(
        Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(),
        height.to_le_bytes().to_vec(),
    );
    state_batch.put_data(
        Keys::state_root(height),
        StateRoot::new_current(height, state_root).to_array(),
    );
    state_batch.put_metadata(AUTHORITATIVE_HIGH_WATER_KEY.to_vec(), legacy_marker.clone());
    state_store
        .commit_maintenance(&state_batch)
        .expect("commit StateService fixture");
    drop(state_store);
    drop(canonical);

    Arguments {
        network_magic,
        mdbx_path,
        pack_path,
        expected_marker_sha256: Crypto::sha256(&legacy_marker),
        report_path,
        max_index_memory_bytes: 16 * 1024 * 1024,
        max_root_nodes: 4,
        max_root_bytes: 1024 * 1024,
    }
}

#[test]
fn parses_explicit_mutating_arguments() {
    let actual = parse_arguments_from(valid_arguments()).expect("parse arguments");
    assert_eq!(actual.network_magic, 0x334F454E);
    assert_eq!(actual.expected_marker_sha256, [0x11; 32]);
    assert_eq!(actual.max_index_memory_bytes, 64 * 1024 * 1024);
    assert_eq!(actual.max_root_nodes, 1234);
    assert_eq!(actual.max_root_bytes, 8 * 1024 * 1024 * 1024);
}

#[test]
fn refuses_implicit_activation() {
    let mut arguments = valid_arguments();
    arguments.pop();
    let error = parse_arguments_from(arguments).expect_err("activation must be explicit");
    assert!(error.to_string().contains("--activate is required"));
}

#[test]
fn validates_exact_marker_digest_length() {
    let mut arguments = valid_arguments();
    let digest = arguments
        .iter()
        .position(|argument| argument == "--expected-marker-sha256")
        .expect("digest option");
    arguments[digest + 1] = "11".repeat(31);
    let error = parse_arguments_from(arguments).expect_err("short digest must fail");
    assert!(error.to_string().contains("31 bytes"));
}

#[test]
fn marker_classification_accepts_only_the_guarded_legacy_or_exact_target() {
    let legacy = b"legacy-marker".to_vec();
    let legacy_sha256 = Crypto::sha256(&legacy);
    let target = current_marker(1);
    assert!(matches!(
        classify_marker(legacy.clone(), legacy_sha256, &target)
            .expect("guarded legacy marker"),
        MarkerState::Legacy(bytes) if bytes == legacy
    ));
    assert!(matches!(
        classify_marker(target.clone(), legacy_sha256, &target).expect("exact activated marker"),
        MarkerState::Activated
    ));

    let conflict = classify_marker(current_marker(2), legacy_sha256, &target)
        .expect_err("different current marker must conflict");
    assert!(conflict.to_string().contains("different current-format"));
    let wrong_legacy = classify_marker(b"other-legacy".to_vec(), legacy_sha256, &target)
        .expect_err("unguarded legacy marker must conflict");
    assert!(wrong_legacy.to_string().contains("SHA-256 differs"));
}

#[test]
fn prepared_report_recovers_every_publication_boundary() {
    let temporary = tempfile::tempdir().expect("temporary report directory");
    let report_path = temporary.path().join("activation.json");
    let prepared_path = temporary_report_path(&report_path).expect("prepared path");
    let staging_path = staging_report_path(&prepared_path).expect("staging path");
    let report = sample_report();

    let absent = inspect_report_files(&report_path, &prepared_path, &report)
        .expect("inspect absent reports");
    assert!(!absent.published && !absent.prepared && absent.report.is_none());

    write_prepared_report(&staging_path, &prepared_path, &report).expect("prepare report");
    assert!(!staging_path.exists());
    let prepared = inspect_report_files(&report_path, &prepared_path, &report)
        .expect("inspect prepared report");
    assert!(!prepared.published && prepared.prepared);
    assert_eq!(prepared.report, Some(report.clone()));

    fs::hard_link(&prepared_path, &report_path).expect("simulate crash after publishing hard link");
    let both = inspect_report_files(&report_path, &prepared_path, &report)
        .expect("inspect both report links");
    assert!(both.published && both.prepared);

    remove_prepared_report(&prepared_path, &report_path).expect("finish recovered publication");
    let published = inspect_report_files(&report_path, &prepared_path, &report)
        .expect("inspect published report");
    assert!(published.published && !published.prepared);
    assert_eq!(published.report, Some(report));
}

#[test]
fn prepared_report_rejects_transition_drift() {
    let temporary = tempfile::tempdir().expect("temporary report directory");
    let report_path = temporary.path().join("activation.json");
    let prepared_path = temporary_report_path(&report_path).expect("prepared path");
    let staging_path = staging_report_path(&prepared_path).expect("staging path");
    let report = sample_report();
    write_prepared_report(&staging_path, &prepared_path, &report).expect("prepare report");

    let mut changed = report;
    changed.block_index += 1;
    let error = inspect_report_files(&report_path, &prepared_path, &changed)
        .expect_err("different transition must reject prepared report");
    assert!(error.to_string().contains("does not bind"));
}

#[test]
fn staging_report_recovery_is_allowed_only_before_marker_commit() {
    let temporary = tempfile::tempdir().expect("temporary report directory");
    let prepared_path = temporary.path().join("activation.json.tmp");
    let staging_path = staging_report_path(&prepared_path).expect("staging path");
    fs::write(&staging_path, b"partial").expect("write interrupted staging report");

    recover_staging_report(
        &staging_path,
        &prepared_path,
        &MarkerState::Legacy(Vec::new()),
    )
    .expect("discard incomplete staging report before marker commit");
    assert!(!staging_path.exists());
    assert!(!prepared_path.exists());

    fs::write(&staging_path, b"fully-written").expect("write staged report");
    File::open(&staging_path)
        .expect("open staged report")
        .sync_all()
        .expect("sync staged report");
    fs::hard_link(&staging_path, &prepared_path).expect("promote prepared hard link");
    recover_staging_report(
        &staging_path,
        &prepared_path,
        &MarkerState::Legacy(Vec::new()),
    )
    .expect("preserve prepared report and remove redundant staging link");
    assert!(!staging_path.exists());
    assert_eq!(
        fs::read(&prepared_path).expect("read prepared report"),
        b"fully-written"
    );

    fs::write(&staging_path, b"impossible-after-commit").expect("write conflicting staging report");
    let error = recover_staging_report(&staging_path, &prepared_path, &MarkerState::Activated)
        .expect_err("post-commit staging state must fail closed");
    assert!(
        error
            .to_string()
            .contains("after the target marker was activated")
    );
}

#[test]
fn relative_report_paths_use_the_current_directory() {
    let report = Path::new("activation.json");
    let prepared = temporary_report_path(report).expect("prepared report path");
    let staging = staging_report_path(&prepared).expect("staging report path");

    assert_eq!(
        report_directory(report).expect("report directory"),
        Path::new(".")
    );
    assert_eq!(prepared, Path::new("activation.json.tmp"));
    assert_eq!(staging, Path::new("activation.json.tmp.staging"));
}

#[cfg(unix)]
#[test]
fn recovered_reports_reject_symlinks_and_fifos_without_following_them() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().expect("temporary report directory");
    let report_path = temporary.path().join("activation.json");
    let prepared_path = temporary_report_path(&report_path).expect("prepared path");
    let external_path = temporary.path().join("external.json");
    let report = sample_report();
    fs::write(
        &external_path,
        serde_json::to_vec(&report).expect("encode external report"),
    )
    .expect("write external report");
    symlink(&external_path, &prepared_path).expect("create prepared-report symlink");

    inspect_report_files(&report_path, &prepared_path, &report)
        .expect_err("prepared-report symlink must not be followed");
    fs::remove_file(&prepared_path).expect("remove prepared-report symlink");

    let fifo = CString::new(prepared_path.as_os_str().as_bytes()).expect("FIFO path CString");
    // SAFETY: `fifo` is a live, NUL-terminated path and the mode contains
    // only standard permission bits. The return value is checked.
    #[allow(unsafe_code)]
    let result = unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) };
    assert_eq!(
        result,
        0,
        "create prepared-report FIFO: {}",
        std::io::Error::last_os_error()
    );
    let error = inspect_report_files(&report_path, &prepared_path, &report)
        .expect_err("prepared-report FIFO must fail without blocking");
    assert!(error.to_string().contains("is not a regular file"));
}

#[test]
fn activation_commits_once_and_recovers_idempotently() {
    let temporary = tempfile::tempdir().expect("temporary activation fixture");
    let arguments = activation_fixture(temporary.path());

    activate(&arguments).expect("activate legacy checkpoint");
    activate(&arguments).expect("reverify already activated checkpoint");

    assert!(arguments.report_path.is_file());
    assert!(
        !temporary_report_path(&arguments.report_path)
            .expect("temporary report path")
            .exists()
    );
    let canonical: Arc<RuntimeStore> = StoreFactory::get_store_with_config(
        "mdbx",
        StorageConfig {
            path: arguments.mdbx_path.clone(),
            read_only: true,
            ..StorageConfig::default()
        },
    )
    .expect("reopen activated MDBX");
    let state_store = canonical
        .open_coordinated_namespace(MDBX_STATE_SERVICE_NAMESPACE)
        .expect("reopen activated StateService namespace");
    let marker = state_store
        .maintenance_metadata(AUTHORITATIVE_HIGH_WATER_KEY)
        .expect("read activated marker")
        .expect("activated marker exists");
    assert!(AuthoritativeHighWaterRecord::decode(&marker).is_ok());
}

#[test]
fn activation_recovers_after_prepared_report_and_post_cas_failures() {
    for (name, boundary, marker_was_committed) in [
        ("prepared", ActivationBoundary::ReportPrepared, false),
        ("committed", ActivationBoundary::MarkerCommitted, true),
    ] {
        let temporary = tempfile::tempdir().expect("temporary activation parent");
        let fixture = temporary.path().join(name);
        fs::create_dir(&fixture).expect("create activation fixture root");
        let arguments = activation_fixture(&fixture);
        let error = activate_with_hook(&arguments, |reached| {
            if reached == boundary {
                bail!("simulated crash at {reached:?}");
            }
            Ok(())
        })
        .expect_err("injected activation boundary must stop the first attempt");
        assert!(error.to_string().contains("simulated crash"));
        assert!(!arguments.report_path.exists());
        assert!(
            temporary_report_path(&arguments.report_path)
                .expect("prepared report path")
                .is_file()
        );

        let canonical: Arc<RuntimeStore> = StoreFactory::get_store_with_config(
            "mdbx",
            StorageConfig {
                path: arguments.mdbx_path.clone(),
                read_only: true,
                ..StorageConfig::default()
            },
        )
        .expect("inspect interrupted activation");
        let state_store = canonical
            .open_coordinated_namespace(MDBX_STATE_SERVICE_NAMESPACE)
            .expect("inspect interrupted StateService marker");
        let marker = state_store
            .maintenance_metadata(AUTHORITATIVE_HIGH_WATER_KEY)
            .expect("read interrupted marker")
            .expect("interrupted marker exists");
        assert_eq!(
            AuthoritativeHighWaterRecord::decode(&marker).is_ok(),
            marker_was_committed
        );
        drop(state_store);
        drop(canonical);

        activate(&arguments).expect("recover interrupted activation");
        assert!(arguments.report_path.is_file());
        assert!(
            !temporary_report_path(&arguments.report_path)
                .expect("prepared report path")
                .exists()
        );
    }
}

#[test]
fn conflicting_current_marker_rejects_before_pack_suffix_recovery() {
    let temporary = tempfile::tempdir().expect("temporary activation fixture");
    let arguments = activation_fixture(temporary.path());
    let checkpoint = PackCheckpoint::read(&arguments.pack_path).expect("read checkpoint");
    let validated = checkpoint
        .validate_authoritative(arguments.network_magic)
        .expect("validate checkpoint");
    let root_internal = validated.source_root_internal();
    let node = Node::new_leaf(b"root-value".to_vec());
    let node_bytes = node.to_array().expect("serialize repeated root node");
    let mut node_key = [0u8; PACK_KEY_BYTES];
    node_key[0] = MPT_NODE_PREFIX;
    node_key[1..].copy_from_slice(&root_internal);
    let pack_config = PackStoreConfig::default()
        .with_max_index_memory_bytes(arguments.max_index_memory_bytes)
        .expect("test pack config");
    let mut pack = PackStore::open(&arguments.pack_path, pack_config).expect("open test pack");
    pack.append_frame(
        PackFrameContext::new(
            checkpoint.source_height,
            checkpoint.source_height,
            root_internal,
            root_internal,
        ),
        &[PackOperation {
            key: node_key,
            kind: PackOpKind::Put(node_bytes),
        }],
    )
    .expect("append valid advanced suffix");
    let advanced_receipt = pack.last_frame_receipt().expect("advanced pack receipt");
    drop(pack);

    let advanced_marker = AuthoritativeHighWaterRecord::new(
        arguments.network_magic,
        validated.store_identity(),
        advanced_receipt,
        checkpoint.source_height,
        root_internal,
    );
    let canonical: Arc<RuntimeStore> = StoreFactory::get_store_with_config(
        "mdbx",
        StorageConfig {
            path: arguments.mdbx_path.clone(),
            ..StorageConfig::default()
        },
    )
    .expect("open fixture MDBX");
    let state_store = canonical
        .open_coordinated_namespace(MDBX_STATE_SERVICE_NAMESPACE)
        .expect("open fixture StateService namespace");
    let mut marker_batch = StoreMaintenanceBatch::new();
    marker_batch.put_metadata(
        AUTHORITATIVE_HIGH_WATER_KEY.to_vec(),
        advanced_marker.encode().to_vec(),
    );
    state_store
        .commit_maintenance(&marker_batch)
        .expect("commit advanced marker");
    drop(state_store);
    drop(canonical);

    let error = activate(&arguments).expect_err("different current marker must conflict");
    assert!(error.to_string().contains("different current-format"));
    let reopened = PackStore::open_at_commit_horizon(
        &arguments.pack_path,
        pack_config,
        Some(advanced_marker.commit_horizon()),
    )
    .expect("advanced pack suffix must remain intact");
    assert_eq!(reopened.last_frame_receipt(), Some(advanced_receipt));
}
