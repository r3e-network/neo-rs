use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};

use tempfile::tempdir;

use super::helpers::{corrupt_payload, open_archive, row, test_config};
use crate::{
    StaticFileArchiveFactory, StaticFileError, StaticFileProvider, StaticFileProviderFactory,
    StaticRecord,
};

#[test]
fn reopen_truncates_a_torn_tail_to_the_last_complete_frame() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let archive = open_archive(&path);
    archive
        .append(StaticRecord::new(0, vec![row(b"a", &[1; 256])]))
        .expect("first record");
    let first_len = std::fs::metadata(&path).expect("metadata").len();
    drop(archive);

    let frame = crate::format::encode_frame(
        StaticRecord::new(1, vec![row(b"b", &[2; 256])]),
        test_config(),
    )
    .expect("encode unpublished frame");
    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open for torn append");
    file.write_all(&frame.bytes[..frame.bytes.len() - 8])
        .expect("append torn unpublished frame");
    file.sync_all().expect("sync torn unpublished frame");
    drop(file);

    let recovered = open_archive(&path);
    assert_eq!(recovered.tip(), Some(0));
    assert_eq!(recovered.get(b"a").expect("lookup"), Some(vec![1; 256]));
    assert_eq!(recovered.get(b"b").expect("lookup"), None);
    assert_eq!(std::fs::metadata(&path).expect("metadata").len(), first_len);
}

#[test]
fn missing_index_does_not_truncate_published_payload_corruption() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let archive = open_archive(&path);
    archive
        .append(StaticRecord::new(0, vec![row(b"first", &[1; 257])]))
        .expect("first record");
    let second_start = std::fs::metadata(&path).expect("metadata").len();
    archive
        .append(StaticRecord::new(1, vec![row(b"second", &[2; 257])]))
        .expect("second record");
    let full_len = std::fs::metadata(&path).expect("metadata").len();
    let index_path = archive.index_path().to_path_buf();
    drop(archive);

    corrupt_payload(&path, second_start);
    std::fs::remove_dir_all(index_path).expect("remove derived index");

    assert!(matches!(
        StaticFileArchiveFactory::default().open(&path),
        Err(StaticFileError::Checksum { .. } | StaticFileError::Compression(_))
    ));
    assert_eq!(std::fs::metadata(&path).expect("metadata").len(), full_len);
}

#[test]
fn interrupted_index_rebuild_keeps_ambiguous_tail_recovery_strict() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let archive = open_archive(&path);
    let prefix = (0..1_024u32)
        .map(|height| StaticRecord::new(height, vec![row(b"shared", &height.to_le_bytes())]))
        .collect();
    archive.append_batch(prefix).expect("published prefix");
    let final_start = std::fs::metadata(&path).expect("metadata").len();
    archive
        .append(StaticRecord::new(
            1_024,
            vec![row(b"shared", &1_024u32.to_le_bytes())],
        ))
        .expect("published final frame");
    let full_len = std::fs::metadata(&path).expect("metadata").len();
    let index_path = archive.index_path().to_path_buf();
    drop(archive);

    corrupt_payload(&path, final_start);
    std::fs::remove_dir_all(index_path).expect("remove derived index");

    for _ in 0..2 {
        assert!(matches!(
            StaticFileArchiveFactory::default().open(&path),
            Err(StaticFileError::Checksum { .. } | StaticFileError::Compression(_))
        ));
        assert_eq!(std::fs::metadata(&path).expect("metadata").len(), full_len);
    }
}

#[test]
fn missing_index_does_not_repair_an_ambiguous_published_torn_tail() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let archive = open_archive(&path);
    archive
        .append_batch(vec![
            StaticRecord::new(0, vec![row(b"first", &[1; 257])]),
            StaticRecord::new(1, vec![row(b"second", &[2; 257])]),
        ])
        .expect("published records");
    let index_path = archive.index_path().to_path_buf();
    let torn_len = std::fs::metadata(&path).expect("metadata").len() - 8;
    drop(archive);

    OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open published archive")
        .set_len(torn_len)
        .expect("tear published footer");
    std::fs::remove_dir_all(index_path).expect("remove derived index");

    assert!(StaticFileArchiveFactory::default().open(&path).is_err());
    assert_eq!(std::fs::metadata(&path).expect("metadata").len(), torn_len);
}

#[test]
fn scrub_rejects_corrupted_complete_key_index() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let archive = open_archive(&path);
    archive
        .append(StaticRecord::new(0, vec![row(b"archive-key", b"value")]))
        .expect("record");
    drop(archive);

    let key_offset =
        u64::try_from(crate::format::FILE_HEADER_LEN + crate::format::FRAME_HEADER_LEN)
            .expect("offset")
            + u64::try_from(crate::format::ROW_INDEX_FIXED_LEN).expect("row prefix");
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("open archive");
    file.seek(SeekFrom::Start(key_offset)).expect("seek key");
    let mut byte = [0u8; 1];
    file.read_exact(&mut byte).expect("read key byte");
    byte[0] ^= 0x80;
    file.seek(SeekFrom::Start(key_offset)).expect("seek key");
    file.write_all(&byte).expect("corrupt key byte");
    file.sync_all().expect("sync corruption");
    drop(file);

    let reopened = StaticFileArchiveFactory::default()
        .open(&path)
        .expect("persistent index allows bounded reopen");
    assert_eq!(
        reopened.get(b"archive-key").expect("payload lookup"),
        Some(b"value".to_vec())
    );
    assert!(matches!(
        reopened.scrub(),
        Err(StaticFileError::Checksum { .. })
    ));
}

#[test]
fn reopen_truncates_an_unpublished_tail_with_corrupted_payload() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let archive = open_archive(&path);
    archive
        .append(StaticRecord::new(0, vec![row(b"first", &[1; 257])]))
        .expect("first record");
    let first_end = std::fs::metadata(&path).expect("metadata").len();
    drop(archive);

    let frame = crate::format::encode_frame(
        StaticRecord::new(1, vec![row(b"second", &[2; 257])]),
        test_config(),
    )
    .expect("encode unpublished tail");
    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open archive for crash simulation");
    file.write_all(&frame.bytes)
        .expect("append unpublished tail");
    file.sync_all().expect("sync unpublished tail");
    drop(file);

    let second_end = corrupt_payload(&path, first_end);
    assert_eq!(
        second_end,
        std::fs::metadata(&path).expect("metadata").len()
    );

    let recovered = open_archive(&path);
    assert_eq!(recovered.tip(), Some(0));
    assert_eq!(recovered.get(b"first").expect("lookup"), Some(vec![1; 257]));
    assert_eq!(recovered.get(b"second").expect("lookup"), None);
    assert_eq!(std::fs::metadata(&path).expect("metadata").len(), first_end);
}

#[test]
fn published_payload_corruption_fails_reads_and_strict_scrub() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let archive = open_archive(&path);
    archive
        .append_batch(vec![
            StaticRecord::new(0, vec![row(b"first", &[1; 257])]),
            StaticRecord::new(1, vec![row(b"second", &[2; 257])]),
        ])
        .expect("records");
    drop(archive);

    corrupt_payload(
        &path,
        u64::try_from(crate::format::FILE_HEADER_LEN).expect("file header length"),
    );

    let reopened = StaticFileArchiveFactory::default()
        .open(&path)
        .expect("persistent index allows bounded reopen");
    assert_eq!(reopened.tip(), Some(1));
    assert!(matches!(
        reopened.get(b"first"),
        Err(StaticFileError::Checksum { .. } | StaticFileError::Compression(_))
    ));
    assert!(matches!(
        reopened.scrub(),
        Err(StaticFileError::Checksum { .. } | StaticFileError::Compression(_))
    ));
}

#[test]
fn published_tail_corruption_is_not_misclassified_as_a_torn_write() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let archive = open_archive(&path);
    archive
        .append(StaticRecord::new(0, vec![row(b"first", &[1; 257])]))
        .expect("first record");
    let first_end = std::fs::metadata(&path).expect("metadata").len();
    archive
        .append(StaticRecord::new(1, vec![row(b"second", &[2; 257])]))
        .expect("second record");
    let full_len = std::fs::metadata(&path).expect("metadata").len();
    drop(archive);

    corrupt_payload(&path, first_end);

    let reopened = open_archive(&path);
    assert_eq!(reopened.tip(), Some(1));
    assert_eq!(std::fs::metadata(&path).expect("metadata").len(), full_len);
    assert!(matches!(
        reopened.get(b"second"),
        Err(StaticFileError::Checksum { .. } | StaticFileError::Compression(_))
    ));
}

#[test]
fn scrub_rejects_extra_persistent_frame_entries() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let archive = open_archive(&path);
    archive
        .append(StaticRecord::new(0, vec![row(b"first", b"value")]))
        .expect("record");
    let header_len = u64::try_from(crate::format::FILE_HEADER_LEN).expect("header length");
    archive
        .insert_test_frame_location(99, header_len, header_len + 1)
        .expect("inject stray frame location");

    assert!(matches!(
        archive.scrub(),
        Err(StaticFileError::InvalidFormat { .. })
    ));
}
