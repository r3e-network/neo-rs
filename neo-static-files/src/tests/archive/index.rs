use std::fs::OpenOptions;
use std::io::Write;

use tempfile::tempdir;

use super::helpers::{open_archive, row, test_config};
use crate::{
    StaticFileArchiveFactory, StaticFileProvider, StaticFileProviderFactory, StaticRecord,
};

#[test]
fn clean_reopen_uses_published_index_without_scanning_payloads() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let factory = StaticFileArchiveFactory::new(test_config());
    let archive = factory.open(&path).expect("create archive");
    archive
        .append_batch(vec![
            StaticRecord::new(0, vec![row(b"a", b"old")]),
            StaticRecord::new(1, vec![row(b"a", b"new"), row(b"b", b"two")]),
        ])
        .expect("append records");
    let index_path = archive.index_path().to_path_buf();
    drop(archive);

    let (reopened, stats) = factory.open_with_stats(&path).expect("reopen archive");

    assert!(
        index_path.is_dir(),
        "MDBX sidecar directory must be durable"
    );
    assert_eq!(reopened.tip(), Some(1));
    assert_eq!(stats.frames_scanned, 0);
    assert_eq!(stats.payloads_decoded, 0);
    assert_eq!(stats.rows_replayed, 0);
    assert!(!stats.index_rebuilt);
    assert_eq!(reopened.get(b"a").expect("lookup"), Some(b"new".to_vec()));
    reopened.scrub().expect("strict archive scrub");
}

#[test]
fn reopen_replays_only_an_archive_suffix_not_published_to_the_index() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let factory = StaticFileArchiveFactory::new(test_config());
    let archive = factory.open(&path).expect("create archive");
    archive
        .append(StaticRecord::new(0, vec![row(b"a", b"one")]))
        .expect("append indexed record");
    drop(archive);

    let frame =
        crate::format::encode_frame(StaticRecord::new(1, vec![row(b"b", b"two")]), test_config())
            .expect("encode unpublished frame");
    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open archive for crash simulation");
    file.write_all(&frame.bytes)
        .expect("append durable archive suffix");
    file.sync_all().expect("sync durable archive suffix");
    drop(file);

    let (recovered, stats) = factory.open_with_stats(&path).expect("recover suffix");

    assert_eq!(recovered.tip(), Some(1));
    assert_eq!(stats.frames_scanned, 1);
    assert_eq!(stats.payloads_decoded, 1);
    assert_eq!(stats.rows_replayed, 1);
    assert!(!stats.index_rebuilt);
    assert_eq!(recovered.get(b"b").expect("lookup"), Some(b"two".to_vec()));
    drop(recovered);

    let (_, clean_stats) = factory.open_with_stats(&path).expect("clean reopen");
    assert_eq!(clean_stats.frames_scanned, 0);
    assert_eq!(clean_stats.payloads_decoded, 0);
}

#[test]
fn reopen_rebuilds_an_index_that_is_ahead_of_the_archive() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let factory = StaticFileArchiveFactory::new(test_config());
    let archive = factory.open(&path).expect("create archive");
    archive
        .append(StaticRecord::new(0, vec![row(b"same", b"zero")]))
        .expect("first record");
    let first_len = std::fs::metadata(&path).expect("metadata").len();
    archive
        .append(StaticRecord::new(1, vec![row(b"same", b"one")]))
        .expect("second record");
    drop(archive);

    OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open archive for truncation")
        .set_len(first_len)
        .expect("simulate archive-first truncation crash");

    let (recovered, stats) = factory.open_with_stats(&path).expect("rebuild index");

    assert!(stats.index_rebuilt);
    assert_eq!(stats.frames_scanned, 1);
    assert_eq!(recovered.tip(), Some(0));
    assert_eq!(
        recovered.get(b"same").expect("lookup"),
        Some(b"zero".to_vec())
    );
}

#[test]
fn missing_index_is_rebuilt_once_then_reopens_without_archive_scan() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let factory = StaticFileArchiveFactory::new(test_config());
    let archive = factory.open(&path).expect("create archive");
    archive
        .append_batch(vec![
            StaticRecord::new(0, vec![row(b"a", b"one")]),
            StaticRecord::new(1, vec![row(b"b", b"two")]),
        ])
        .expect("append records");
    let index_path = archive.index_path().to_path_buf();
    drop(archive);
    std::fs::remove_dir_all(&index_path).expect("remove derived index");

    let (rebuilt, stats) = factory
        .open_with_stats(&path)
        .expect("rebuild missing index");
    assert!(stats.index_rebuilt);
    assert_eq!(stats.frames_scanned, 2);
    assert_eq!(rebuilt.get(b"b").expect("lookup"), Some(b"two".to_vec()));
    drop(rebuilt);

    let (_, clean_stats) = factory.open_with_stats(&path).expect("clean reopen");
    assert_eq!(clean_stats.frames_scanned, 0);
    assert!(!clean_stats.index_rebuilt);
}

#[test]
fn archive_identity_discards_a_sidecar_from_a_replaced_archive() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let factory = StaticFileArchiveFactory::new(test_config());
    let archive = factory.open(&path).expect("create archive");
    archive
        .append(StaticRecord::new(0, vec![row(b"old", b"value")]))
        .expect("append old archive");
    drop(archive);
    std::fs::remove_file(&path).expect("replace authoritative archive");

    let (replacement, stats) = factory.open_with_stats(&path).expect("open replacement");

    assert!(stats.index_rebuilt);
    assert_eq!(replacement.tip(), None);
    assert_eq!(replacement.get(b"old").expect("lookup"), None);
    replacement.scrub().expect("replacement scrub");
}

#[test]
fn repeated_reopen_and_truncate_preserve_latest_value_per_key() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let mut expected = [None, None, None];
    let mut archive = open_archive(&path);

    for height in 0..12u32 {
        let slot = usize::try_from(height % 3).expect("slot");
        let key = [b'a' + u8::try_from(slot).expect("slot byte")];
        let value = height.to_le_bytes();
        archive
            .append(StaticRecord::new(height, vec![row(&key, &value)]))
            .expect("append generated row");
        expected[slot] = Some(value);
        drop(archive);
        archive = open_archive(&path);
        for (slot, expected) in expected.iter().enumerate() {
            let key = [b'a' + u8::try_from(slot).expect("slot byte")];
            assert_eq!(archive.get(&key).expect("lookup"), expected.map(Vec::from));
        }
    }

    archive
        .truncate_after(Some(5))
        .expect("truncate generated archive");
    drop(archive);
    let archive = open_archive(&path);
    for slot in 0..3u8 {
        let key = [b'a' + slot];
        let height = u32::from(slot) + 3;
        assert_eq!(
            archive.get(&key).expect("lookup"),
            Some(height.to_le_bytes().to_vec())
        );
    }
    archive.scrub().expect("generated archive scrub");
}

#[test]
fn latest_heights_for_keys_preserves_input_order_and_missing_entries() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let factory = StaticFileArchiveFactory::new(test_config());
    let archive = factory.open(&path).expect("create archive");
    archive
        .append_batch(vec![
            StaticRecord::new(
                0,
                vec![row(b"same", b"zero"), row(b"zero-only", b"zero-only-value")],
            ),
            StaticRecord::new(
                1,
                vec![row(b"same", b"one"), row(b"one-only", b"one-only-value")],
            ),
            StaticRecord::new(
                2,
                vec![row(b"same", b"two"), row(b"two-only", b"two-only-value")],
            ),
        ])
        .expect("append records");
    drop(archive);

    let reopened = factory.open(&path).expect("reopen archive");
    let keys = [
        b"same".as_slice(),
        b"missing".as_slice(),
        b"zero-only".as_slice(),
        b"same".as_slice(),
        b"one-only".as_slice(),
        b"two-only".as_slice(),
    ];

    assert_eq!(
        reopened
            .latest_heights_for_keys(&keys)
            .expect("resolve latest heights"),
        vec![Some(2), None, Some(0), Some(2), Some(1), Some(2)]
    );
}

#[test]
fn frame_row_keys_reads_a_frame_index_without_payload_lookup() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let factory = StaticFileArchiveFactory::new(test_config());
    let archive = factory.open(&path).expect("create archive");
    archive
        .append_batch(vec![
            StaticRecord::new(0, vec![row(b"seed", b"zero")]),
            StaticRecord::new(
                1,
                vec![row(b"zeta", b"z"), row(b"alpha", b"a"), row(b"mid", b"m")],
            ),
        ])
        .expect("append records");
    drop(archive);

    let reopened = factory.open(&path).expect("reopen archive");

    assert_eq!(
        reopened.frame_row_keys(1).expect("enumerate frame keys"),
        Some(vec![b"alpha".to_vec(), b"mid".to_vec(), b"zeta".to_vec()])
    );
    assert_eq!(reopened.frame_row_keys(3).expect("missing frame"), None);
}
