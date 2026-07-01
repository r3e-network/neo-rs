//! Static-file append store contract tests.

use neo_static_files::{
    FileStaticFileProvider, FileStaticFiles, Offset, Segment, StaticFileFactory, StaticFiles,
};
use std::{fs::OpenOptions, io::Write};

#[test]
fn append_returns_stable_offsets_and_reads_payloads() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = FileStaticFiles::open(temp.path()).expect("open static files");

    let first = store
        .append(Segment::Blocks, 42, b"block-42")
        .expect("append first block");
    let second = store
        .append(Segment::Blocks, 43, b"block-43")
        .expect("append second block");

    assert_eq!(first.offset, Offset::ZERO);
    assert!(first.next_offset <= second.offset);
    assert!(second.next_offset > second.offset);
    assert_eq!(
        store
            .read(Segment::Blocks, first.offset)
            .expect("read first"),
        b"block-42"
    );
    assert_eq!(
        store
            .read(Segment::Blocks, second.offset)
            .expect("read second"),
        b"block-43"
    );
}

#[test]
fn static_file_factory_creates_file_provider_by_alias() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store =
        StaticFileFactory::get_static_files("file", temp.path()).expect("open static files");
    let record = store
        .append(Segment::Blocks, 11, b"factory-block")
        .expect("append via factory store");

    assert_eq!(
        store.read(Segment::Blocks, record.offset).expect("read"),
        b"factory-block"
    );
    assert!(
        StaticFileFactory::get_static_file_provider("FileStaticFiles")
            .expect("file static provider")
            .as_any()
            .is::<FileStaticFileProvider>()
    );
}

#[test]
fn static_file_factory_rejects_unknown_provider() {
    let temp = tempfile::tempdir().expect("tempdir");
    let err = match StaticFileFactory::get_static_files("typoed-static-files", temp.path()) {
        Ok(_) => panic!("unknown static-file provider must be rejected"),
        Err(err) => err,
    };

    assert!(
        err.to_string().contains("typoed-static-files"),
        "error should name the unknown provider: {err}"
    );
}

#[test]
fn truncate_to_committed_end_removes_orphan_tail() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = FileStaticFiles::open(temp.path()).expect("open static files");

    let committed = store
        .append(Segment::Receipts, 100, b"committed")
        .expect("append committed");
    let orphan = store
        .append(Segment::Receipts, 101, b"orphan")
        .expect("append orphan");

    store
        .truncate_to(Segment::Receipts, committed.next_offset)
        .expect("truncate orphan tail");

    assert_eq!(
        store
            .read(Segment::Receipts, committed.offset)
            .expect("read committed"),
        b"committed"
    );
    assert!(
        store.read(Segment::Receipts, orphan.offset).is_err(),
        "orphan record after the committed end must not remain visible"
    );
}

#[test]
fn partial_tail_record_fails_closed_until_truncated() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = FileStaticFiles::open(temp.path()).expect("open static files");

    let committed = store
        .append(Segment::Transactions, 500, b"committed-tx")
        .expect("append committed tx");
    let partial_offset = committed.next_offset;
    let path = temp.path().join("transactions.nsf");
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .expect("open segment for partial tail");
    file.write_all(b"NSF1partial")
        .expect("write partial record tail");
    file.flush().expect("flush partial record tail");

    assert!(
        store.read(Segment::Transactions, partial_offset).is_err(),
        "partial tail record must fail closed before recovery truncation"
    );

    store
        .truncate_to(Segment::Transactions, committed.next_offset)
        .expect("truncate partial tail");

    assert_eq!(
        store
            .read(Segment::Transactions, committed.offset)
            .expect("read committed tx"),
        b"committed-tx"
    );
    assert!(
        store.read(Segment::Transactions, partial_offset).is_err(),
        "partial tail must not remain visible after truncation"
    );
}

#[test]
fn truncate_segments_to_replays_hot_watermark_recovery() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = FileStaticFiles::open(temp.path()).expect("open static files");

    let block = store
        .append(Segment::Blocks, 800, b"committed-block")
        .expect("append committed block");
    let tx = store
        .append(Segment::Transactions, 800, b"committed-tx")
        .expect("append committed tx");
    let orphan_block = store
        .append(Segment::Blocks, 801, b"orphan-block")
        .expect("append orphan block");
    let orphan_tx = store
        .append(Segment::Transactions, 801, b"orphan-tx")
        .expect("append orphan tx");

    store
        .truncate_segments_to(&[
            (Segment::Blocks, block.next_offset),
            (Segment::Transactions, tx.next_offset),
        ])
        .expect("recover cold files to committed ends");

    assert_eq!(
        store
            .read(Segment::Blocks, block.offset)
            .expect("read committed block"),
        b"committed-block"
    );
    assert_eq!(
        store
            .read(Segment::Transactions, tx.offset)
            .expect("read committed tx"),
        b"committed-tx"
    );
    assert!(store.read(Segment::Blocks, orphan_block.offset).is_err());
    assert!(store.read(Segment::Transactions, orphan_tx.offset).is_err());
}

#[test]
fn append_and_fsync_batch_returns_offsets_for_hot_indexes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = FileStaticFiles::open(temp.path()).expect("open static files");

    let records = store
        .append_and_fsync_batch(&[
            (Segment::Blocks, 900, b"block-900".as_slice()),
            (Segment::Transactions, 900, b"tx-900".as_slice()),
            (Segment::Receipts, 900, b"receipt-900".as_slice()),
        ])
        .expect("append and fsync cold batch");

    assert_eq!(records.len(), 3);
    assert_eq!(records[0].segment, Segment::Blocks);
    assert_eq!(records[1].segment, Segment::Transactions);
    assert_eq!(records[2].segment, Segment::Receipts);
    assert_eq!(
        store
            .read(records[0].segment, records[0].offset)
            .expect("read block"),
        b"block-900"
    );
    assert_eq!(
        store
            .read(records[1].segment, records[1].offset)
            .expect("read tx"),
        b"tx-900"
    );
    assert_eq!(
        store
            .read(records[2].segment, records[2].offset)
            .expect("read receipt"),
        b"receipt-900"
    );
}

#[test]
fn segment_paths_are_isolated() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = FileStaticFiles::open(temp.path()).expect("open static files");

    let block = store
        .append(Segment::Blocks, 7, b"block")
        .expect("append block");
    let tx = store
        .append(Segment::Transactions, 7, b"tx")
        .expect("append tx");

    assert_eq!(block.offset, Offset::ZERO);
    assert_eq!(tx.offset, Offset::ZERO);
    assert_eq!(
        store
            .read(Segment::Blocks, block.offset)
            .expect("read block"),
        b"block"
    );
    assert_eq!(
        store
            .read(Segment::Transactions, tx.offset)
            .expect("read tx"),
        b"tx"
    );
}
