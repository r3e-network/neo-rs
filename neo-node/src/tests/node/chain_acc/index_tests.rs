//! Correctness and recovery tests for the persistent `chain.acc` offset index.

use std::ffi::OsString;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use neo_payloads::block::Block;
use neo_primitives::UInt256;

use super::format::{read_chain_acc_header, read_next_chain_acc_block};
use super::format_tests::{encode_prefixed_chain_acc, linked_empty_blocks};
use super::index::{ChainAccResumeValidation, position_chain_acc_reader};

fn index_path(archive_path: &Path) -> PathBuf {
    let mut path: OsString = archive_path.as_os_str().to_owned();
    path.push(".idx");
    PathBuf::from(path)
}

fn write_archive(path: &Path, start_height: u32, blocks: &[Block]) {
    std::fs::write(path, encode_prefixed_chain_acc(start_height, blocks))
        .expect("write chain.acc fixture");
}

fn position_and_read(
    path: &Path,
    blocks: &[Block],
    target_record: usize,
    expected_prev_hash: &UInt256,
) -> (bool, bool, u64, Block) {
    let file = File::open(path).expect("open chain.acc fixture");
    let mut reader = BufReader::with_capacity(64, file);
    let header = read_chain_acc_header(&mut reader).expect("read chain.acc header");
    let position = position_chain_acc_reader(
        &mut reader,
        Some(path),
        header,
        target_record,
        ChainAccResumeValidation {
            has_first_record: true,
            expected_height: Some(blocks[target_record].index()),
            expected_prev_hash: Some(expected_prev_hash),
        },
    )
    .expect("position chain.acc reader");
    let mut block_bytes = Vec::new();
    let block = read_next_chain_acc_block(&mut reader, target_record, &mut block_bytes)
        .expect("read resumed block");
    (
        position.index_hit,
        position.index_rebuilt,
        position.offset,
        block,
    )
}

#[test]
fn persistent_index_reuses_an_exact_validated_frontier() {
    let directory = tempfile::tempdir().expect("temp directory");
    let archive = directory.path().join("chain.0.acc");
    let blocks = linked_empty_blocks(0, 6);
    write_archive(&archive, 0, &blocks);

    let first = position_and_read(&archive, &blocks, 3, &blocks[2].hash());
    assert!(!first.0, "the first positioning pass must build the index");
    assert!(first.1);
    assert_eq!(first.3.index(), 3);

    let second = position_and_read(&archive, &blocks, 3, &blocks[2].hash());
    assert!(second.0, "the exact persisted frontier should be reused");
    assert!(!second.1);
    assert_eq!(second.2, first.2);
    assert_eq!(second.3.hash(), blocks[3].hash());

    let sidecar = index_path(&archive);
    assert!(sidecar.is_file());
    assert!(
        sidecar.metadata().expect("index metadata").len() < 1 << 20,
        "small archives must produce small bounded indexes"
    );
    let temporary_files = std::fs::read_dir(directory.path())
        .expect("read temp directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".idx.tmp."))
        .count();
    assert_eq!(temporary_files, 0, "atomic publication left a temp file");
}

#[test]
fn noop_import_does_not_create_or_load_a_sidecar() {
    let directory = tempfile::tempdir().expect("temp directory");
    let archive = directory.path().join("chain.0.acc");
    let blocks = linked_empty_blocks(0, 2);
    write_archive(&archive, 0, &blocks);

    let file = File::open(&archive).expect("open chain.acc fixture");
    let mut reader = BufReader::with_capacity(64, file);
    let header = read_chain_acc_header(&mut reader).expect("read chain.acc header");
    let position = position_chain_acc_reader(
        &mut reader,
        Some(&archive),
        header,
        0,
        ChainAccResumeValidation {
            has_first_record: false,
            expected_height: None,
            expected_prev_hash: None,
        },
    )
    .expect("position no-op import");

    assert!(position.offset_index.is_none());
    assert!(!position.index_hit);
    assert!(!position.index_rebuilt);
    assert!(!index_path(&archive).exists());
}

#[test]
fn import_observations_advance_the_durable_frontier() {
    let directory = tempfile::tempdir().expect("temp directory");
    let archive = directory.path().join("chain.0.acc");
    let blocks = linked_empty_blocks(0, 6);
    write_archive(&archive, 0, &blocks);

    let file = File::open(&archive).expect("open chain.acc fixture");
    let mut reader = BufReader::with_capacity(64, file);
    let header = read_chain_acc_header(&mut reader).expect("read chain.acc header");
    let position = position_chain_acc_reader(
        &mut reader,
        Some(&archive),
        header,
        0,
        ChainAccResumeValidation {
            has_first_record: true,
            expected_height: Some(0),
            expected_prev_hash: None,
        },
    )
    .expect("position at archive start");
    let mut session = position.offset_index.expect("index recording session");
    let mut block_bytes = Vec::new();
    for (record, expected_block) in blocks.iter().enumerate().take(4) {
        let block = read_next_chain_acc_block(&mut reader, record, &mut block_bytes)
            .expect("read indexed block");
        assert_eq!(block.hash(), expected_block.hash());
        session.observe_record(record + 1, block_bytes.len());
    }
    session.persist_best_effort();
    drop(reader);

    let resumed = position_and_read(&archive, &blocks, 4, &blocks[3].hash());
    assert!(resumed.0, "recorded exact frontier should be an index hit");
    assert!(!resumed.1);
    assert_eq!(resumed.3.hash(), blocks[4].hash());
}

#[test]
fn corrupt_checksum_falls_back_and_repairs_the_sidecar() {
    let directory = tempfile::tempdir().expect("temp directory");
    let archive = directory.path().join("chain.0.acc");
    let blocks = linked_empty_blocks(0, 6);
    write_archive(&archive, 0, &blocks);
    let _ = position_and_read(&archive, &blocks, 3, &blocks[2].hash());

    let sidecar = index_path(&archive);
    let mut bytes = std::fs::read(&sidecar).expect("read index");
    let last = bytes.last_mut().expect("index checksum byte");
    *last ^= 0xff;
    std::fs::write(&sidecar, bytes).expect("corrupt index checksum");

    let resumed = position_and_read(&archive, &blocks, 3, &blocks[2].hash());
    assert!(!resumed.0);
    assert!(resumed.1, "corrupt index should be rebuilt sequentially");
    assert_eq!(resumed.3.hash(), blocks[3].hash());

    let repaired = position_and_read(&archive, &blocks, 3, &blocks[2].hash());
    assert!(repaired.0, "replacement sidecar should pass its checksum");
}

#[test]
fn replaced_archive_invalidates_the_bound_identity() {
    let directory = tempfile::tempdir().expect("temp directory");
    let archive = directory.path().join("chain.0.acc");
    let first_blocks = linked_empty_blocks(0, 6);
    write_archive(&archive, 0, &first_blocks);
    let _ = position_and_read(&archive, &first_blocks, 3, &first_blocks[2].hash());

    let replacement_blocks = linked_empty_blocks(10, 6);
    write_archive(&archive, 10, &replacement_blocks);
    let resumed = position_and_read(
        &archive,
        &replacement_blocks,
        3,
        &replacement_blocks[2].hash(),
    );

    assert!(!resumed.0);
    assert!(
        resumed.1,
        "archive identity mismatch should rebuild the index"
    );
    assert_eq!(resumed.3.index(), 13);
    assert_eq!(resumed.3.hash(), replacement_blocks[3].hash());
}

#[test]
fn ledger_link_mismatch_rejects_a_valid_offset_before_use() {
    let directory = tempfile::tempdir().expect("temp directory");
    let archive = directory.path().join("chain.0.acc");
    let blocks = linked_empty_blocks(0, 6);
    write_archive(&archive, 0, &blocks);
    let _ = position_and_read(&archive, &blocks, 3, &blocks[2].hash());

    let wrong_ledger_hash = UInt256::from([0x5a; 32]);
    let resumed = position_and_read(&archive, &blocks, 3, &wrong_ledger_hash);

    assert!(!resumed.0, "a link mismatch must not count as an index hit");
    assert!(
        resumed.1,
        "a link mismatch must force sequential positioning"
    );
    assert_eq!(
        resumed.3.prev_hash(),
        &blocks[2].hash(),
        "fallback must leave the canonical archive block for normal validation"
    );
}

#[test]
fn oversized_sidecar_is_rejected_without_unbounded_allocation() {
    let directory = tempfile::tempdir().expect("temp directory");
    let archive = directory.path().join("chain.0.acc");
    let blocks = linked_empty_blocks(0, 6);
    write_archive(&archive, 0, &blocks);
    let sidecar = index_path(&archive);
    let file = File::create(&sidecar).expect("create oversized sparse index");
    file.set_len(33 * 1_024 * 1_024)
        .expect("size oversized sparse index");
    drop(file);

    let resumed = position_and_read(&archive, &blocks, 3, &blocks[2].hash());
    assert!(!resumed.0);
    assert!(resumed.1);
    assert_eq!(resumed.3.hash(), blocks[3].hash());
    assert!(
        sidecar.metadata().expect("replacement metadata").len() < 1 << 20,
        "oversized sidecar should be replaced by a bounded index"
    );
}
