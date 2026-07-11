use tempfile::tempdir;

use super::helpers::{open_archive, row};
use crate::{
    StaticFileArchiveFactory, StaticFileConfig, StaticFileError, StaticFileProvider,
    StaticFileProviderFactory, StaticRecord,
};

#[test]
fn append_lookup_and_reopen_preserve_latest_rows() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let archive = open_archive(&path);

    archive
        .append_batch(vec![
            StaticRecord::new(0, vec![row(b"a", b"old"), row(b"b", b"two")]),
            StaticRecord::new(1, vec![row(b"a", b"new"), row(b"c", b"three")]),
        ])
        .expect("append contiguous records");

    assert_eq!(archive.tip(), Some(1));
    assert_eq!(archive.get(b"a").expect("lookup"), Some(b"new".to_vec()));
    assert_eq!(archive.get(b"b").expect("lookup"), Some(b"two".to_vec()));
    assert_eq!(archive.get(b"missing").expect("lookup"), None);
    drop(archive);

    let reopened = open_archive(&path);
    assert_eq!(reopened.tip(), Some(1));
    assert_eq!(reopened.get(b"a").expect("lookup"), Some(b"new".to_vec()));
    assert_eq!(reopened.get(b"c").expect("lookup"), Some(b"three".to_vec()));
}

#[test]
fn append_rejects_duplicate_gap_and_duplicate_rows() {
    let temp = tempdir().expect("tempdir");
    let archive = open_archive(&temp.path().join("ledger.static"));
    archive
        .append(StaticRecord::new(0, vec![row(b"a", b"one")]))
        .expect("first record");

    assert!(matches!(
        archive.append(StaticRecord::new(0, vec![row(b"b", b"two")])),
        Err(StaticFileError::NonContiguous {
            expected: 1,
            actual: 0
        })
    ));
    assert!(matches!(
        archive.append(StaticRecord::new(2, vec![row(b"b", b"two")])),
        Err(StaticFileError::NonContiguous {
            expected: 1,
            actual: 2
        })
    ));
    assert!(matches!(
        archive.append(StaticRecord::new(
            1,
            vec![row(b"same", b"one"), row(b"same", b"two")],
        )),
        Err(StaticFileError::DuplicateKey { .. })
    ));
    assert_eq!(archive.tip(), Some(0));
}

#[test]
fn truncate_after_restores_prior_row_versions_and_persists_the_result() {
    let temp = tempdir().expect("tempdir");
    let archive = open_archive(&temp.path().join("ledger.static"));
    archive
        .append_batch(vec![
            StaticRecord::new(0, vec![row(b"same", b"zero")]),
            StaticRecord::new(1, vec![row(b"same", b"one")]),
            StaticRecord::new(2, vec![row(b"last", b"two")]),
        ])
        .expect("records");

    archive.truncate_after(Some(0)).expect("truncate archive");

    assert_eq!(archive.tip(), Some(0));
    assert_eq!(
        archive.get(b"same").expect("lookup"),
        Some(b"zero".to_vec())
    );
    assert_eq!(archive.get(b"last").expect("lookup"), None);

    let path = archive.path().to_path_buf();
    drop(archive);
    let reopened = open_archive(&path);
    assert_eq!(reopened.tip(), Some(0));
    assert_eq!(
        reopened.get(b"same").expect("lookup"),
        Some(b"zero".to_vec())
    );
    assert_eq!(reopened.get(b"last").expect("lookup"), None);
    reopened.scrub().expect("truncated archive scrub");
}

#[test]
fn factory_rejects_invalid_resource_and_compression_limits() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let mut config = StaticFileConfig::default();
    config.cache_capacity = 0;
    assert!(matches!(
        StaticFileArchiveFactory::new(config).open(&path),
        Err(StaticFileError::InvalidFormat { .. })
    ));

    let mut config = StaticFileConfig::default();
    config.compression_level = i32::MAX;
    assert!(matches!(
        StaticFileArchiveFactory::new(config).open(&path),
        Err(StaticFileError::InvalidFormat { .. })
    ));
}
