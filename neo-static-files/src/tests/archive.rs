use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};

use tempfile::tempdir;

use crate::{
    StaticFileArchiveFactory, StaticFileConfig, StaticFileError, StaticFileProvider,
    StaticFileProviderFactory, StaticRecord, StaticRow,
};

fn row(key: &[u8], value: &[u8]) -> StaticRow {
    StaticRow::new(key.to_vec(), value.to_vec())
}

fn open_archive(path: &std::path::Path) -> crate::StaticFileArchive {
    StaticFileArchiveFactory::new(StaticFileConfig {
        compression_level: 1,
        cache_capacity: 4,
        ..StaticFileConfig::default()
    })
    .open(path)
    .expect("open archive")
}

fn frame_layout(path: &std::path::Path, frame_offset: u64) -> (u64, u64) {
    let mut file = OpenOptions::new()
        .read(true)
        .open(path)
        .expect("open archive");
    file.seek(SeekFrom::Start(frame_offset))
        .expect("seek frame header");
    let mut header_bytes = [0u8; crate::format::FRAME_HEADER_LEN];
    file.read_exact(&mut header_bytes)
        .expect("read frame header");
    let header = crate::format::decode_frame_header(&header_bytes, frame_offset)
        .expect("decode frame header");
    let payload_offset = frame_offset
        + u64::try_from(crate::format::FRAME_HEADER_LEN).expect("header length")
        + u64::from(header.index_len);
    (payload_offset, frame_offset + header.frame_len)
}

fn corrupt_payload(path: &std::path::Path, frame_offset: u64) -> u64 {
    let (payload_offset, frame_end) = frame_layout(path, frame_offset);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .expect("open archive for corruption");
    file.seek(SeekFrom::Start(payload_offset))
        .expect("seek payload");
    let mut byte = [0u8; 1];
    file.read_exact(&mut byte).expect("read payload byte");
    byte[0] ^= 0x80;
    file.seek(SeekFrom::Start(payload_offset))
        .expect("seek payload");
    file.write_all(&byte).expect("corrupt payload byte");
    file.sync_all().expect("sync corruption");
    frame_end
}

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
fn writer_lease_is_held_until_the_last_archive_clone_drops() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let factory = StaticFileArchiveFactory::default();
    let archive = factory.open(&path).expect("first writer");
    let clone = archive.clone();

    assert!(
        factory.open(&path).is_err(),
        "a second writer must not open the same archive"
    );
    drop(archive);
    assert!(
        factory.open(&path).is_err(),
        "a provider clone must keep the writer lease alive"
    );
    drop(clone);

    factory
        .open(&path)
        .expect("kernel lease should release with the final clone");
}

#[cfg(unix)]
#[test]
fn writer_lease_canonicalizes_symlinked_archive_paths() {
    let temp = tempdir().expect("tempdir");
    let real = temp.path().join("real");
    std::fs::create_dir(&real).expect("real archive directory");
    let alias = temp.path().join("alias");
    std::os::unix::fs::symlink(&real, &alias).expect("archive directory symlink");
    let factory = StaticFileArchiveFactory::default();
    let archive = factory
        .open(&real.join("ledger.static"))
        .expect("open canonical path");

    assert!(matches!(
        factory.open(&alias.join("ledger.static")),
        Err(StaticFileError::WriterOwned { .. })
    ));
    drop(archive);
    factory
        .open(&alias.join("ledger.static"))
        .expect("alias should open after lease release");
}

#[test]
fn writer_lease_follows_hard_link_file_identity() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let alias = temp.path().join("ledger-alias.static");
    let factory = StaticFileArchiveFactory::default();
    let archive = factory.open(&path).expect("open archive");
    std::fs::hard_link(&path, &alias).expect("hard-link archive alias");

    assert!(matches!(
        factory.open(&alias),
        Err(StaticFileError::WriterOwned { .. })
    ));
    drop(archive);
    factory
        .open(&alias)
        .expect("hard-link alias should open after lease release");
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
fn reopen_truncates_a_torn_tail_to_the_last_complete_frame() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let archive = open_archive(&path);
    archive
        .append(StaticRecord::new(0, vec![row(b"a", &[1; 256])]))
        .expect("first record");
    let first_len = std::fs::metadata(&path).expect("metadata").len();
    archive
        .append(StaticRecord::new(1, vec![row(b"b", &[2; 256])]))
        .expect("second record");
    let full_len = std::fs::metadata(&path).expect("metadata").len();
    assert!(full_len > first_len + 8);
    drop(archive);

    OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open for truncation")
        .set_len(full_len - 8)
        .expect("tear footer");

    let recovered = open_archive(&path);
    assert_eq!(recovered.tip(), Some(0));
    assert_eq!(recovered.get(b"a").expect("lookup"), Some(vec![1; 256]));
    assert_eq!(recovered.get(b"b").expect("lookup"), None);
    assert_eq!(std::fs::metadata(&path).expect("metadata").len(), first_len);
}

#[test]
fn reopen_rejects_corrupted_complete_key_index() {
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

    assert!(matches!(
        StaticFileArchiveFactory::default().open(&path),
        Err(StaticFileError::Checksum { .. })
    ));
}

#[test]
fn reopen_truncates_a_complete_tail_with_corrupted_payload() {
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
    drop(archive);

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
fn reopen_rejects_corrupted_interior_payload() {
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

    assert!(matches!(
        StaticFileArchiveFactory::default().open(&path),
        Err(StaticFileError::Checksum { .. } | StaticFileError::Compression(_))
    ));
}

#[test]
fn truncate_after_rebuilds_tip_and_latest_key_index() {
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
