use super::*;
use crate::mdbx::{
    MDBX_REBASE_INCOMPLETE_FILE, MdbxExactKeyExclusion, MdbxRebaseOptions, finalize_mdbx_rebase,
    rebase_mdbx_environment,
};

const STATE_TABLE: &str = "neo_state_service";
const MAINTENANCE_TABLE: &str = "neo_node_metadata";

fn rebase_options(source: &std::path::Path, destination: &std::path::Path) -> MdbxRebaseOptions {
    let mut options = MdbxRebaseOptions::new(
        source,
        destination,
        vec![MAINTENANCE_TABLE.to_owned(), STATE_TABLE.to_owned()],
        MdbxExactKeyExclusion::new(STATE_TABLE, vec![0xf0], 33),
    );
    options.batch_scanned_rows = 2;
    options.batch_retained_bytes = 32;
    options.geometry_upper_bytes = 64 * 1024 * 1024;
    options.geometry_growth_bytes = 1024 * 1024;
    options
}

fn seed_source(path: &std::path::Path, extra_table: bool) {
    let provider = MdbxStoreProvider::new(StorageConfig {
        path: path.to_path_buf(),
        mdbx_geometry_upper_bytes: Some(64 * 1024 * 1024),
        mdbx_geometry_growth_bytes: Some(1024 * 1024),
        ..Default::default()
    });
    let mut canonical = provider
        .get_mdbx_store(std::path::Path::new(""))
        .expect("open source canonical");
    canonical
        .put(vec![0x01], b"canonical-one".to_vec())
        .expect("write canonical row");
    canonical
        .put(b"ordinary-ascii-key".to_vec(), b"not-a-table".to_vec())
        .expect("write UTF-8 canonical row");

    let mut state = canonical
        .open_named_table(STATE_TABLE)
        .expect("open state table");
    let mut exact_node = vec![0u8; 33];
    exact_node[0] = 0xf0;
    state
        .put(exact_node, b"obsolete-node".to_vec())
        .expect("write exact node");
    state
        .put(vec![0xf0, 0x01], b"short-future-metadata".to_vec())
        .expect("write short f0 metadata");
    let mut long_f0 = vec![0x22; 34];
    long_f0[0] = 0xf0;
    state
        .put(long_f0, b"long-future-metadata".to_vec())
        .expect("write long f0 metadata");
    state
        .put(vec![0xf1, 0x09], b"root-metadata".to_vec())
        .expect("write root metadata");

    let mut maintenance = StoreMaintenanceBatch::new();
    maintenance.put_metadata(b"authoritative-marker".to_vec(), b"epoch-7".to_vec());
    canonical
        .commit_maintenance(&maintenance)
        .expect("write maintenance marker");
    if extra_table {
        canonical
            .open_named_table("future_service")
            .expect("create unexpected table");
    }
}

#[test]
fn rebase_excludes_only_exact_node_keys_and_verifies_every_table() {
    let temporary = TempDir::new().expect("tempdir");
    let source = temporary.path().join("source");
    let destination = temporary.path().join("destination");
    seed_source(&source, false);

    let report =
        rebase_mdbx_environment(&rebase_options(&source, &destination)).expect("verified rebase");
    assert_eq!(
        report.named_tables,
        vec![MAINTENANCE_TABLE.to_owned(), STATE_TABLE.to_owned()]
    );
    assert_ne!(
        report.source_environment_id,
        report.destination_environment_id
    );
    assert_eq!(report.source_environment_id.len(), 32);
    assert_eq!(report.destination_environment_id.len(), 32);
    assert!(destination.join(MDBX_REBASE_INCOMPLETE_FILE).exists());
    let state_report = report
        .tables
        .iter()
        .find(|table| table.table == STATE_TABLE)
        .expect("state table report");
    assert_eq!(state_report.source_rows, 4);
    assert_eq!(state_report.copied_rows, 3);
    assert_eq!(state_report.destination_rows, 3);
    assert_eq!(state_report.excluded_rows, 1);
    assert_eq!(state_report.ordered_sha256.len(), 64);
    let main_report = report
        .tables
        .iter()
        .find(|table| table.table == "main")
        .expect("main table report");
    assert_eq!(main_report.descriptor_rows, 2);
    assert_eq!(
        main_report.destination_rows,
        main_report.copied_rows + main_report.descriptor_rows
    );

    let provider = MdbxStoreProvider::new(StorageConfig {
        path: destination.clone(),
        ..Default::default()
    });
    assert!(
        provider.get_mdbx_store(std::path::Path::new("")).is_err(),
        "normal open must wait for evidence publication"
    );
    finalize_mdbx_rebase(&destination).expect("publish verified destination");
    assert!(!destination.join(MDBX_REBASE_INCOMPLETE_FILE).exists());
    let canonical = provider
        .get_mdbx_store(std::path::Path::new(""))
        .expect("open rebased canonical");
    assert_eq!(
        canonical.try_get_bytes(b"ordinary-ascii-key"),
        Some(b"not-a-table".to_vec())
    );
    assert_eq!(
        canonical
            .maintenance_metadata(b"authoritative-marker")
            .expect("read marker"),
        Some(b"epoch-7".to_vec())
    );
    let state = canonical
        .open_named_table(STATE_TABLE)
        .expect("open rebased state");
    let mut exact_node = vec![0u8; 33];
    exact_node[0] = 0xf0;
    assert_eq!(state.try_get_bytes(&exact_node), None);
    assert_eq!(
        state.try_get_bytes(&[0xf0, 0x01]),
        Some(b"short-future-metadata".to_vec())
    );
    let mut long_f0 = vec![0x22; 34];
    long_f0[0] = 0xf0;
    assert_eq!(
        state.try_get_bytes(&long_f0),
        Some(b"long-future-metadata".to_vec())
    );
}

#[test]
fn rebase_rejects_an_unknown_named_table_before_creating_destination() {
    let temporary = TempDir::new().expect("tempdir");
    let source = temporary.path().join("source");
    let destination = temporary.path().join("destination");
    seed_source(&source, true);

    let error = rebase_mdbx_environment(&rebase_options(&source, &destination))
        .expect_err("unknown table must fail closed");
    assert!(error.to_string().contains("future_service"), "{error}");
    assert!(!destination.exists());
}

#[test]
fn normal_store_rejects_an_incomplete_rebase_sentinel() {
    let temporary = TempDir::new().expect("tempdir");
    let path = temporary.path().join("incomplete");
    fs::create_dir(&path).expect("create incomplete directory");
    fs::write(path.join(MDBX_REBASE_INCOMPLETE_FILE), b"incomplete\n")
        .expect("write incomplete sentinel");
    let provider = MdbxStoreProvider::new(StorageConfig {
        path,
        ..Default::default()
    });
    let error = provider
        .get_mdbx_store(std::path::Path::new(""))
        .expect_err("incomplete rebase must not open");
    assert!(error.to_string().contains("incomplete rebase"), "{error}");
}

#[test]
fn rebase_rejects_a_destination_nested_inside_the_source() {
    let temporary = TempDir::new().expect("tempdir");
    let source = temporary.path().join("source");
    seed_source(&source, false);
    let destination = source.join("nested-destination");
    let error = rebase_mdbx_environment(&rebase_options(&source, &destination))
        .expect_err("nested destination must fail closed");
    assert!(error.to_string().contains("must not contain"), "{error}");
    assert!(!destination.exists());
}

#[test]
fn rebase_rejects_an_incomplete_rebase_as_its_source() {
    let temporary = TempDir::new().expect("tempdir");
    let source = temporary.path().join("source");
    seed_source(&source, false);
    fs::write(source.join(MDBX_REBASE_INCOMPLETE_FILE), b"incomplete\n")
        .expect("mark source incomplete");
    let destination = temporary.path().join("destination");
    let error = rebase_mdbx_environment(&rebase_options(&source, &destination))
        .expect_err("incomplete source must fail closed");
    assert!(error.to_string().contains("incomplete rebase"), "{error}");
    assert!(!destination.exists());
}
