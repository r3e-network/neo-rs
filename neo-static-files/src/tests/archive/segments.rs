use std::fs::OpenOptions;

use tempfile::tempdir;

use super::helpers::{row, test_config};
use crate::{
    StaticFileArchiveFactory, StaticFileProvider, StaticFileProviderFactory, StaticRecord,
};

fn rotating_factory() -> StaticFileArchiveFactory {
    let mut config = test_config();
    config.max_segment_bytes =
        u64::try_from(crate::format::FILE_HEADER_LEN).expect("header length fits u64") + 1;
    StaticFileArchiveFactory::new(config)
}

#[test]
fn rotation_routes_reads_without_cross_segment_cache_aliasing() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let factory = rotating_factory();
    let archive = factory.open(&path).expect("create archive");

    archive
        .append_batch(vec![
            StaticRecord::new(0, vec![row(b"a", b"first")]),
            StaticRecord::new(1, vec![row(b"b", b"other")]),
            StaticRecord::new(2, vec![row(b"a", b"latest")]),
        ])
        .expect("append rotating records");

    assert_eq!(archive.segment_count(), 3);
    assert_eq!(
        archive.get(b"b").expect("second segment"),
        Some(b"other".to_vec())
    );
    assert_eq!(
        archive.get(b"a").expect("third segment"),
        Some(b"latest".to_vec())
    );
    assert_eq!(
        archive.frame_row_keys(0).expect("first frame keys"),
        Some(vec![b"a".to_vec()])
    );
    archive.scrub().expect("multi-segment scrub");

    drop(archive);
    let (reopened, stats) = factory.open_with_stats(&path).expect("clean reopen");
    assert_eq!(stats.segments_retained, 3);
    assert_eq!(stats.frames_scanned, 0);
    assert_eq!(reopened.tip(), Some(2));
    assert_eq!(reopened.segment_count(), 3);
    assert_eq!(
        reopened.get(b"a").expect("reopened lookup"),
        Some(b"latest".to_vec())
    );
}

#[test]
fn staged_rotation_is_invisible_until_one_index_publication() {
    let temp = tempdir().expect("tempdir");
    let archive = rotating_factory()
        .open(&temp.path().join("ledger.static"))
        .expect("create archive");

    archive
        .stage_append(vec![
            StaticRecord::new(0, vec![row(b"zero", b"0")]),
            StaticRecord::new(1, vec![row(b"one", b"1")]),
            StaticRecord::new(2, vec![row(b"two", b"2")]),
        ])
        .expect("stage across segments");

    assert_eq!(archive.segment_count(), 3);
    assert_eq!(archive.tip(), None);
    assert_eq!(archive.get(b"two").expect("unpublished lookup"), None);

    archive
        .publish_staged_append()
        .expect("publish one global index transaction");
    assert_eq!(archive.tip(), Some(2));
    assert_eq!(
        archive.get(b"two").expect("published lookup"),
        Some(b"2".to_vec())
    );
}

#[test]
fn recovery_keeps_complete_suffix_segments_and_discards_a_torn_final_one() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let factory = rotating_factory();
    let archive = factory.open(&path).expect("create archive");
    archive
        .append(StaticRecord::new(0, vec![row(b"zero", b"0")]))
        .expect("published prefix");
    archive
        .stage_append(vec![
            StaticRecord::new(1, vec![row(b"one", b"1")]),
            StaticRecord::new(2, vec![row(b"two", b"2")]),
        ])
        .expect("unpublished suffix");
    let segment_paths = archive.segment_paths();
    let torn_path = segment_paths.last().expect("final segment").clone();
    drop(archive);

    let torn_len = std::fs::metadata(&torn_path).expect("torn metadata").len() - 8;
    OpenOptions::new()
        .write(true)
        .open(&torn_path)
        .expect("open final segment")
        .set_len(torn_len)
        .expect("tear final segment");

    let (recovered, stats) = factory.open_with_stats(&path).expect("recover segments");
    assert_eq!(recovered.tip(), Some(1));
    assert_eq!(recovered.segment_count(), 2);
    assert_eq!(stats.segments_retained, 2);
    assert_eq!(stats.frames_scanned, 1);
    assert!(stats.archive_tail_truncated);
    assert!(!torn_path.exists());
    assert_eq!(
        recovered.get(b"one").expect("retained suffix"),
        Some(b"1".to_vec())
    );
    assert_eq!(recovered.get(b"two").expect("discarded tail"), None);
    recovered.scrub().expect("recovered scrub");
}

#[test]
fn truncation_removes_later_segments_and_restores_prior_row_versions() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let factory = rotating_factory();
    let archive = factory.open(&path).expect("create archive");
    archive
        .append_batch(vec![
            StaticRecord::new(0, vec![row(b"same", b"zero")]),
            StaticRecord::new(1, vec![row(b"same", b"one")]),
            StaticRecord::new(2, vec![row(b"same", b"two")]),
        ])
        .expect("append segments");
    let removed_paths = archive
        .segment_paths()
        .into_iter()
        .skip(1)
        .collect::<Vec<_>>();

    archive
        .truncate_after(Some(0))
        .expect("truncate to genesis segment");

    assert_eq!(archive.tip(), Some(0));
    assert_eq!(archive.segment_count(), 1);
    assert_eq!(
        archive.get(b"same").expect("rolled back value"),
        Some(b"zero".to_vec())
    );
    assert!(removed_paths.iter().all(|path| !path.exists()));
    drop(archive);

    let reopened = factory.open(&path).expect("reopen truncation");
    assert_eq!(reopened.segment_count(), 1);
    assert_eq!(
        reopened.get(b"same").expect("reopened value"),
        Some(b"zero".to_vec())
    );
    reopened.scrub().expect("truncated scrub");
}

#[test]
fn missing_global_index_rebuilds_all_segments_once() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let factory = rotating_factory();
    let archive = factory.open(&path).expect("create archive");
    archive
        .append_batch(vec![
            StaticRecord::new(0, vec![row(b"zero", b"0")]),
            StaticRecord::new(1, vec![row(b"one", b"1")]),
            StaticRecord::new(2, vec![row(b"two", b"2")]),
        ])
        .expect("append segments");
    let index_path = archive.index_path().to_path_buf();
    drop(archive);
    std::fs::remove_dir_all(index_path).expect("remove derived index");

    let (rebuilt, stats) = factory
        .open_with_stats(&path)
        .expect("rebuild all segments");
    assert!(stats.index_rebuilt);
    assert_eq!(stats.frames_scanned, 3);
    assert_eq!(
        rebuilt.get(b"two").expect("rebuilt lookup"),
        Some(b"2".to_vec())
    );
    drop(rebuilt);

    let (_, clean_stats) = factory
        .open_with_stats(&path)
        .expect("clean indexed reopen");
    assert!(!clean_stats.index_rebuilt);
    assert_eq!(clean_stats.frames_scanned, 0);
}

#[test]
fn index_ahead_of_a_missing_final_segment_rebuilds_the_contiguous_prefix() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let factory = rotating_factory();
    let archive = factory.open(&path).expect("create archive");
    archive
        .append_batch(vec![
            StaticRecord::new(0, vec![row(b"zero", b"0")]),
            StaticRecord::new(1, vec![row(b"one", b"1")]),
            StaticRecord::new(2, vec![row(b"two", b"2")]),
        ])
        .expect("append segments");
    let final_segment = archive
        .segment_paths()
        .last()
        .expect("final segment")
        .clone();
    drop(archive);
    std::fs::remove_file(final_segment).expect("simulate durable segment rollback");

    let (rebuilt, stats) = factory
        .open_with_stats(&path)
        .expect("rebuild prefix index");

    assert!(stats.index_rebuilt);
    assert_eq!(stats.frames_scanned, 2);
    assert_eq!(rebuilt.tip(), Some(1));
    assert_eq!(rebuilt.segment_count(), 2);
    assert_eq!(
        rebuilt.get(b"one").expect("retained value"),
        Some(b"1".to_vec())
    );
    assert_eq!(rebuilt.get(b"two").expect("removed value"), None);
}

#[test]
fn startup_removes_an_unpublished_pending_segment_header() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let factory = rotating_factory();
    drop(factory.open(&path).expect("create archive"));
    let pending = temp.path().join("ledger.static.segment-0000000001.pending");
    std::fs::write(&pending, b"incomplete header").expect("write pending segment");

    let reopened = factory.open(&path).expect("clean pending segment");

    assert_eq!(reopened.tip(), None);
    assert_eq!(reopened.segment_count(), 1);
    assert!(!pending.exists());
}

#[test]
fn renamed_segment_height_fails_closed() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let factory = rotating_factory();
    let archive = factory.open(&path).expect("create archive");
    archive
        .append_batch(vec![
            StaticRecord::new(0, vec![row(b"zero", b"0")]),
            StaticRecord::new(1, vec![row(b"one", b"1")]),
        ])
        .expect("append segments");
    let rotated = archive.segment_paths()[1].clone();
    let renamed = temp.path().join("ledger.static.segment-0000000002");
    drop(archive);
    std::fs::rename(rotated, &renamed).expect("rename segment to wrong height");

    let error = factory
        .open(&path)
        .expect_err("segment gap must reject startup");

    assert!(error.to_string().contains("non-contiguous"), "{error}");
}

#[test]
fn segment_from_another_archive_identity_is_rejected() {
    let temp = tempdir().expect("tempdir");
    let first_path = temp.path().join("ledger.static");
    let second_path = temp.path().join("other.static");
    let factory = rotating_factory();
    drop(factory.open(&first_path).expect("create first archive"));
    drop(factory.open(&second_path).expect("create second archive"));
    std::fs::copy(
        &second_path,
        temp.path().join("ledger.static.segment-0000000001"),
    )
    .expect("copy foreign segment");

    let error = factory
        .open(&first_path)
        .expect_err("foreign segment identity must reject startup");

    assert!(error.to_string().contains("identity mismatch"), "{error}");
}

#[test]
fn malformed_reserved_segment_name_is_rejected() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let factory = rotating_factory();
    drop(factory.open(&path).expect("create archive"));
    std::fs::write(
        temp.path().join("ledger.static.segment-manual-backup"),
        b"not a segment",
    )
    .expect("write malformed reserved file");

    let error = factory
        .open(&path)
        .expect_err("reserved segment namespace must be strict");

    assert!(
        error.to_string().contains("malformed archive segment"),
        "{error}"
    );
}
