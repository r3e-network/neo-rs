//! # Chain Accumulator Import Tests
//!
//! Regression tests for `chain.acc` import helpers.
//!
//! ## Boundary
//!
//! These tests exercise the node-level stream driver, batching, range checks,
//! and reports through typed blockchain handles.
//!
//! ## Contents
//!
//! Fixtures and tests for format validation, batching, throughput accounting,
//! stop heights, resume behavior, and canonical import failures.

use super::batch::{
    ChainAccImportComposition, PendingChainAccBatch, import_chain_acc_batch, take_import_batch,
};
use super::driver::{
    import_chain_acc_from_reader_until_height, import_chain_acc_report_from_reader_until_height,
};
use super::format_tests::{
    empty_block, empty_block_with_prev_hash, encode_chain_acc, linked_empty_blocks,
};
use super::range::{
    bounded_chain_acc_import_range, chain_acc_import_record_count, count_only_stop_height_exceeded,
    count_only_stop_height_reached,
};
use super::*;
use neo_blockchain::{BlockchainCommand, BlockchainHandle, ImportBlocksReply};
use neo_payloads::block::Block;
use neo_payloads::{Signer, Transaction, Witness};
use neo_primitives::{UInt160, WitnessScope};
use neo_storage::persistence::providers::memory_store::MemoryStore;
use std::sync::Arc;

fn signed_test_transaction(nonce: u32) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_script(vec![neo_vm::OpCode::RET.byte()]);
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
    tx.set_witnesses(vec![Witness::new_with_scripts(
        Vec::new(),
        vec![neo_vm::OpCode::PUSH1.byte()],
    )]);
    tx
}

fn non_empty_block_with_prev_hash(
    index: u32,
    prev_hash: neo_primitives::UInt256,
    transactions: Vec<Transaction>,
) -> Block {
    let mut header = neo_payloads::Header::new();
    header.set_index(index);
    header.set_prev_hash(prev_hash);
    let mut block = Block::from_parts(header, transactions);
    block.try_rebuild_merkle_root().expect("merkle root");
    block
}

fn memory_store_with_ledger_tip(tip: u32, hash: neo_primitives::UInt256) -> Arc<MemoryStore> {
    use neo_storage::{StorageItem, StorageKey};

    let store: Arc<MemoryStore> = Arc::new(MemoryStore::new());
    let mut cache = neo_storage::persistence::StoreCache::new_from_store(Arc::clone(&store), false);
    let current = neo_native_contracts::LedgerContract::new()
        .serialize_hash_index_state(&hash, tip)
        .expect("serialize current ledger pointer");
    cache.data_cache().add(
        StorageKey::new(neo_native_contracts::LedgerContract::ID, vec![12]),
        StorageItem::from_bytes(current),
    );
    cache.try_commit().expect("commit test Ledger tip");
    store
}

fn pending_batch_is_empty_only(batch: &PendingChainAccBatch) -> bool {
    batch.len > 0 && batch.composition.empty_blocks > 0 && batch.composition.transaction_blocks == 0
}

async fn import_chain_acc_from_reader<R>(
    handle: &BlockchainHandle,
    reader: &mut R,
    path: Option<&std::path::Path>,
    verify: bool,
    expected_range: Option<ChainAccExpectedRange>,
    storage: Option<Arc<MemoryStore>>,
) -> anyhow::Result<u64>
where
    R: std::io::Read + std::io::Seek,
{
    Ok(
        import_chain_acc_from_reader_report(handle, reader, path, verify, expected_range, storage)
            .await?
            .imported,
    )
}

async fn import_chain_acc_from_reader_report<R>(
    handle: &BlockchainHandle,
    reader: &mut R,
    path: Option<&std::path::Path>,
    verify: bool,
    expected_range: Option<ChainAccExpectedRange>,
    storage: Option<Arc<MemoryStore>>,
) -> anyhow::Result<ChainAccImportReport>
where
    R: std::io::Read + std::io::Seek,
{
    import_chain_acc_report_from_reader_until_height(
        handle,
        reader,
        path,
        verify,
        expected_range,
        None,
        storage,
    )
    .await
}

#[test]
fn take_import_batch_preserves_preallocated_capacity_when_more_blocks_remain() {
    let mut batch = Vec::with_capacity(IMPORT_BATCH_SIZE);
    batch.push(empty_block(1));

    let imported = take_import_batch(&mut batch, true);

    assert_eq!(imported.len(), 1);
    assert_eq!(batch.len(), 0);
    assert!(
        batch.capacity() >= IMPORT_BATCH_SIZE,
        "expected to preserve batch capacity, got {}",
        batch.capacity()
    );
}

#[test]
fn take_import_batch_avoids_reallocating_after_final_flush() {
    let mut batch = Vec::with_capacity(IMPORT_BATCH_SIZE);
    batch.push(empty_block(1));

    let imported = take_import_batch(&mut batch, false);

    assert_eq!(imported.len(), 1);
    assert_eq!(batch.len(), 0);
    assert_eq!(batch.capacity(), 0);
}

#[test]
fn bounded_chain_acc_import_range_caps_only_within_expected_range() {
    let full = ChainAccExpectedRange {
        start_height: 10,
        end_height: 20,
    };

    assert_eq!(
        bounded_chain_acc_import_range(Some(full), None, None),
        Some(full)
    );
    assert_eq!(
        bounded_chain_acc_import_range(Some(full), None, Some(15)),
        Some(ChainAccExpectedRange {
            start_height: 10,
            end_height: 15,
        })
    );
    assert_eq!(
        bounded_chain_acc_import_range(Some(full), None, Some(25)),
        Some(full)
    );
    assert_eq!(
        bounded_chain_acc_import_range(Some(full), None, Some(9)),
        None
    );
    assert_eq!(bounded_chain_acc_import_range(None, None, Some(15)), None);
    assert_eq!(
        bounded_chain_acc_import_range(None, Some(0), Some(15)),
        Some(ChainAccExpectedRange {
            start_height: 0,
            end_height: 15,
        })
    );
}

#[test]
fn chain_acc_import_record_count_uses_bounded_expected_range() {
    let full = ChainAccExpectedRange {
        start_height: 10,
        end_height: 20,
    };
    let bounded = ChainAccExpectedRange {
        start_height: 10,
        end_height: 15,
    };

    assert_eq!(
        chain_acc_import_record_count(11, Some(full), Some(bounded), Some(15))
            .expect("bounded count"),
        6
    );
    assert_eq!(
        chain_acc_import_record_count(11, Some(full), None, Some(9)).expect("below-range stop"),
        0
    );
    assert_eq!(
        chain_acc_import_record_count(11, None, None, None).expect("unbounded count"),
        11
    );
    assert_eq!(
        chain_acc_import_record_count(11, None, Some(bounded), Some(15))
            .expect("prefixed count-only bound"),
        6
    );
    assert_eq!(
        chain_acc_import_record_count(11, None, None, Some(15))
            .expect("unprefixed count-only bound"),
        11
    );
}

#[test]
fn count_only_stop_height_reached_only_applies_without_expected_range() {
    let full = ChainAccExpectedRange {
        start_height: 10,
        end_height: 20,
    };

    assert!(count_only_stop_height_reached(None, Some(2), 2));
    assert!(count_only_stop_height_reached(None, Some(2), 3));
    assert!(!count_only_stop_height_reached(None, Some(2), 1));
    assert!(!count_only_stop_height_reached(None, None, 2));
    assert!(!count_only_stop_height_reached(Some(full), Some(15), 15));
    assert!(count_only_stop_height_exceeded(None, Some(2), 3));
    assert!(!count_only_stop_height_exceeded(None, Some(2), 2));
    assert!(!count_only_stop_height_exceeded(Some(full), Some(15), 16));
}

#[tokio::test]
async fn import_chain_acc_can_stop_count_only_file_before_full_end() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let bytes = encode_chain_acc(&linked_empty_blocks(0, 5));
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            panic!("expected import blocks command");
        };
        let heights = import
            .blocks
            .iter()
            .map(|block| block.index())
            .collect::<Vec<_>>();
        assert_eq!(heights, vec![0, 1, 2]);
        reply
            .send(ImportBlocksReply::ok(import.blocks.len()))
            .expect("reply import");
        assert!(
            commands.try_recv().is_err(),
            "stop height should prevent importing blocks beyond the bound"
        );
    });

    let imported = import_chain_acc_from_reader_until_height(
        &handle,
        &mut cursor,
        None,
        false,
        None,
        Some(2),
        None::<Arc<neo_storage::persistence::providers::memory_store::MemoryStore>>,
    )
    .await
    .expect("count-only import should stop at requested height");

    service.await.expect("service task");
    assert_eq!(imported, 3);
}

#[tokio::test]
async fn import_chain_acc_until_height_public_wrapper_bounds_count_only_file() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let temp = tempfile::NamedTempFile::new().expect("temp chain.acc");
    std::fs::write(temp.path(), encode_chain_acc(&linked_empty_blocks(0, 5)))
        .expect("write chain.acc");
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            panic!("expected import blocks command");
        };
        let heights = import
            .blocks
            .iter()
            .map(|block| block.index())
            .collect::<Vec<_>>();
        assert_eq!(heights, vec![0, 1, 2]);
        reply
            .send(ImportBlocksReply::ok(import.blocks.len()))
            .expect("reply import");
    });

    let imported = import_chain_acc_until_height(
        &handle,
        temp.path(),
        false,
        Some(2),
        None::<Arc<neo_storage::persistence::providers::memory_store::MemoryStore>>,
    )
    .await
    .expect("bounded public import should stop at requested height");

    service.await.expect("service task");
    assert_eq!(imported, 3);
}

#[tokio::test]
async fn import_chain_acc_errors_when_blockchain_accepts_partial_batch() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let bytes = encode_chain_acc(&linked_empty_blocks(0, 2));
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            panic!("expected import blocks command");
        };
        assert_eq!(import.blocks.len(), 2);
        reply
            .send(ImportBlocksReply::ok(1))
            .expect("reply partial import");
    });

    let err = import_chain_acc_from_reader(&handle, &mut cursor, None, false, None, None)
        .await
        .expect_err("partial import must be an error");

    service.await.expect("service task");
    assert!(
        err.to_string().contains("partial chain.acc import"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("imported 1 of 2"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn chain_acc_batch_preflight_rejects_bad_block_version_before_import_command() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let mut bad_block = empty_block(0);
    bad_block.header.set_version(1);

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        import_chain_acc_batch(
            &handle,
            vec![bad_block],
            ChainAccImportComposition::default(),
            None,
            true,
        ),
    )
    .await
    .expect("preflight should return before waiting for an import reply");
    let err = match result {
        Ok(_) => panic!("bad block version must fail preflight"),
        Err(err) => err,
    };

    assert!(
        err.to_string().contains("preflight"),
        "unexpected error: {err}"
    );
    assert!(
        commands.try_recv().is_err(),
        "preflight failure must skip the ImportBlocks command"
    );
}

#[tokio::test]
async fn import_chain_acc_errors_when_blockchain_finalization_fails() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let bytes = encode_chain_acc(&linked_empty_blocks(0, 2));
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            panic!("expected import blocks command");
        };
        assert_eq!(import.blocks.len(), 2);
        reply
            .send(ImportBlocksReply::failed(
                2,
                "state-root worker reported a failed operation",
            ))
            .expect("reply failed finalization");
    });

    let err = import_chain_acc_from_reader(&handle, &mut cursor, None, false, None, None)
        .await
        .expect_err("finalization failure must be an error");

    service.await.expect("service task");
    assert!(
        err.to_string().contains("finalization failed"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("state-root worker"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn import_chain_acc_rejects_expected_range_count_mismatch_before_import() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let bytes = encode_chain_acc(&[empty_block(0)]);
    let mut cursor = std::io::Cursor::new(bytes);

    let err = import_chain_acc_from_reader(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 0,
            end_height: 1,
        }),
        None,
    )
    .await
    .expect_err("count mismatch must be an error");

    assert!(
        commands.try_recv().is_err(),
        "range validation must fail before import"
    );
    assert!(
        err.to_string().contains("count mismatch"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn import_chain_acc_rejects_expected_range_first_height_mismatch_before_import() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let bytes = encode_chain_acc(&[empty_block(1)]);
    let mut cursor = std::io::Cursor::new(bytes);

    let err = import_chain_acc_from_reader(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 0,
            end_height: 0,
        }),
        None,
    )
    .await
    .expect_err("first height mismatch must be an error");

    assert!(
        commands.try_recv().is_err(),
        "range validation must fail before import"
    );
    assert!(
        err.to_string().contains("first block height mismatch"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn import_chain_acc_rejects_non_contiguous_heights_before_import() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let bytes = encode_chain_acc(&[empty_block(0), empty_block(2)]);
    let mut cursor = std::io::Cursor::new(bytes);

    let err = import_chain_acc_from_reader(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 0,
            end_height: 1,
        }),
        None,
    )
    .await
    .expect_err("height gap must be an error");

    assert!(
        commands.try_recv().is_err(),
        "range validation must fail before import"
    );
    assert!(
        err.to_string().contains("not contiguous"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn import_chain_acc_rejects_partial_range_prev_hash_mismatch_before_import() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let local_tip_hash = neo_primitives::UInt256::from([0xAA; 32]);
    let store = memory_store_with_ledger_tip(9, local_tip_hash);
    let wrong_prev_hash = neo_primitives::UInt256::from([0xBB; 32]);
    let bytes = encode_chain_acc(&[empty_block_with_prev_hash(10, wrong_prev_hash)]);
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            return false;
        };
        let imported = import.blocks.len();
        reply
            .send(ImportBlocksReply::ok(imported))
            .expect("reply import");
        true
    });

    let result = import_chain_acc_from_reader(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 10,
            end_height: 10,
        }),
        Some(store),
    )
    .await;
    service.abort();

    assert!(
        result.is_err(),
        "partial package with mismatched previous hash must fail, got {result:?}"
    );
    let err = result.expect_err("partial package with mismatched previous hash must fail");
    assert!(
        err.to_string().contains("previous hash mismatch"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn import_chain_acc_rejects_internal_prev_hash_mismatch_before_import() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let genesis = empty_block(0);
    let wrong_prev_hash = neo_primitives::UInt256::from([0xCC; 32]);
    let bytes = encode_chain_acc(&[genesis, empty_block_with_prev_hash(1, wrong_prev_hash)]);
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Ok(Some(BlockchainCommand::ImportBlocks { import, reply })) =
            tokio::time::timeout(std::time::Duration::from_millis(50), commands.recv()).await
        else {
            return false;
        };
        let imported = import.blocks.len();
        reply
            .send(ImportBlocksReply::ok(imported))
            .expect("reply import");
        true
    });

    let result = import_chain_acc_from_reader(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 0,
            end_height: 1,
        }),
        None,
    )
    .await;
    let import_reached_service = service.await.expect("service task");

    assert!(
        result.is_err(),
        "internal previous-hash mismatch must fail before import, got {result:?}"
    );
    assert!(
        !import_reached_service,
        "internal previous-hash validation must fail before sending an import command"
    );
    let err = result.expect_err("internal previous-hash mismatch must fail");
    assert!(
        err.to_string().contains("previous hash mismatch"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("record 1"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn import_chain_acc_allows_partial_range_prev_hash_match() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let local_tip_hash = neo_primitives::UInt256::from([0xAA; 32]);
    let store = memory_store_with_ledger_tip(9, local_tip_hash);
    let bytes = encode_chain_acc(&[empty_block_with_prev_hash(10, local_tip_hash)]);
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            panic!("expected import blocks command");
        };
        assert_eq!(import.blocks.len(), 1);
        assert_eq!(import.blocks[0].index(), 10);
        reply.send(ImportBlocksReply::ok(1)).expect("reply import");
    });

    let imported = import_chain_acc_from_reader(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 10,
            end_height: 10,
        }),
        Some(store),
    )
    .await
    .expect("matching previous hash can import");

    service.await.expect("service task");
    assert_eq!(imported, 1);
}

#[tokio::test]
async fn import_chain_acc_can_stop_before_full_expected_range_end() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let bytes = encode_chain_acc(&linked_empty_blocks(0, 5));
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            panic!("expected import blocks command");
        };
        let heights = import
            .blocks
            .iter()
            .map(|block| block.index())
            .collect::<Vec<_>>();
        assert_eq!(heights, vec![0, 1, 2]);
        reply
            .send(ImportBlocksReply::ok(import.blocks.len()))
            .expect("reply import");
        assert!(
            commands.try_recv().is_err(),
            "stop height should prevent importing blocks beyond the bound"
        );
    });

    let imported = import_chain_acc_from_reader_until_height(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 0,
            end_height: 4,
        }),
        Some(2),
        None::<Arc<neo_storage::persistence::providers::memory_store::MemoryStore>>,
    )
    .await
    .expect("bounded expected-range import should stop at requested height");

    service.await.expect("service task");
    assert_eq!(imported, 3);
}

#[tokio::test]
async fn import_chain_acc_resumes_full_expected_range_after_local_tip() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let blocks = linked_empty_blocks(0, 5);
    let local_tip_hash = blocks[2].hash();
    let store = memory_store_with_ledger_tip(2, local_tip_hash);
    let bytes = encode_chain_acc(&blocks);
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            panic!("expected import blocks command");
        };
        let heights = import
            .blocks
            .iter()
            .map(|block| block.index())
            .collect::<Vec<_>>();
        assert_eq!(heights, vec![3, 4]);
        reply
            .send(ImportBlocksReply::ok(import.blocks.len()))
            .expect("reply import");
        assert!(
            commands.try_recv().is_err(),
            "resuming after local tip should not import earlier package blocks"
        );
    });

    let imported = import_chain_acc_from_reader(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 0,
            end_height: 4,
        }),
        Some(store),
    )
    .await
    .expect("full package import should resume after local tip");

    service.await.expect("service task");
    assert_eq!(imported, 2);
}

#[tokio::test]
async fn import_chain_acc_report_tracks_last_imported_tip() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let blocks = linked_empty_blocks(0, 3);
    let expected_tip = LocalLedgerTip {
        height: blocks[2].index(),
        hash: blocks[2].hash(),
    };
    let bytes = encode_chain_acc(&blocks);
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            panic!("expected import blocks command");
        };
        assert_eq!(import.blocks.len(), 3);
        reply
            .send(ImportBlocksReply::ok(import.blocks.len()))
            .expect("reply import");
    });

    let report = import_chain_acc_from_reader_report(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 0,
            end_height: 2,
        }),
        None,
    )
    .await
    .expect("import report");

    service.await.expect("service task");
    assert_eq!(report.imported, 3);
    assert_eq!(report.last_imported_tip, Some(expected_tip));
}

#[tokio::test]
async fn import_chain_acc_report_tracks_final_average_bps() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let bytes = encode_chain_acc(&linked_empty_blocks(0, 3));
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            panic!("expected import blocks command");
        };
        assert_eq!(import.blocks.len(), 3);
        reply
            .send(ImportBlocksReply::ok(import.blocks.len()))
            .expect("reply import");
    });

    let report = import_chain_acc_from_reader_report(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 0,
            end_height: 2,
        }),
        None,
    )
    .await
    .expect("import report");

    service.await.expect("service task");
    assert_eq!(report.imported, 3);
    assert!(report.elapsed_seconds >= 0.0);
    assert!(
        report.average_blocks_per_second > 0.0,
        "importing blocks should report a positive final BPS, got {report:?}"
    );
}

#[tokio::test]
async fn import_chain_acc_report_tracks_empty_and_transaction_bearing_blocks() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let genesis = empty_block(0);
    let block1 =
        non_empty_block_with_prev_hash(1, genesis.hash(), vec![signed_test_transaction(1)]);
    let block2 = empty_block_with_prev_hash(2, block1.hash());
    let blocks = vec![genesis, block1, block2];
    let bytes = encode_chain_acc(&blocks);
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            panic!("expected import blocks command");
        };
        assert_eq!(import.blocks.len(), 3);
        reply
            .send(ImportBlocksReply::ok_with_stats(
                import.blocks.len(),
                neo_blockchain::ImportBlocksStats {
                    empty_blocks: 2,
                    empty_elapsed: std::time::Duration::from_millis(2),
                    transaction_blocks: 1,
                    transaction_elapsed: std::time::Duration::from_millis(1),
                    transaction_block_clone_elapsed: std::time::Duration::from_millis(3),
                    transaction_ledger_insert_elapsed: std::time::Duration::from_millis(4),
                    transaction_finalized_delivery_elapsed: std::time::Duration::from_millis(5),
                    finalization_elapsed: std::time::Duration::from_millis(1),
                    finalization_commit_handlers_elapsed: std::time::Duration::from_micros(600),
                    finalization_store_commit_elapsed: std::time::Duration::from_micros(400),
                },
            ))
            .expect("reply import");
    });

    let report = import_chain_acc_from_reader_report(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 0,
            end_height: 2,
        }),
        None,
    )
    .await
    .expect("import report");

    service.await.expect("service task");
    assert_eq!(report.imported, 3);
    assert_eq!(report.empty_blocks, 2);
    assert_eq!(report.empty_only_blocks, 0);
    assert!(report.empty_blocks_per_second > 0.0);
    assert_eq!(report.transaction_blocks, 1);
    assert_eq!(report.transactions, 1);
    assert_eq!(report.transaction_block_clone_seconds, 0.003);
    assert_eq!(report.transaction_ledger_insert_seconds, 0.004);
    assert_eq!(report.transaction_finalized_delivery_seconds, 0.005);
    assert_eq!(report.finalization_seconds, 0.001);
    assert_eq!(report.finalization_commit_handlers_seconds, 0.0006);
    assert_eq!(report.finalization_store_commit_seconds, 0.0004);
    assert!(
        report.transaction_blocks_per_second > 0.0,
        "transaction-bearing BPS must be reported independently from empty-block throughput"
    );
}

#[tokio::test]
async fn import_chain_acc_report_times_only_transaction_bearing_batches_for_transaction_bps() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let empty_run = neo_blockchain::empty_block_fast_forward::MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS;
    let mut blocks = linked_empty_blocks(0, empty_run);
    let prev = blocks.last().expect("previous block");
    blocks.push(non_empty_block_with_prev_hash(
        empty_run as u32,
        prev.hash(),
        vec![signed_test_transaction(1)],
    ));
    let bytes = encode_chain_acc(&blocks);
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            panic!("expected import blocks command");
        };
        assert_eq!(import.blocks.len(), empty_run + 1);
        assert!(
            import.blocks[..empty_run]
                .iter()
                .all(|block| block.transactions.is_empty())
        );
        assert_eq!(import.blocks[empty_run].transactions.len(), 1);
        reply
            .send(ImportBlocksReply::ok_with_stats(
                import.blocks.len(),
                neo_blockchain::ImportBlocksStats {
                    empty_blocks: empty_run,
                    empty_elapsed: std::time::Duration::from_millis(20),
                    transaction_blocks: 1,
                    transaction_elapsed: std::time::Duration::from_millis(1),
                    transaction_block_clone_elapsed: std::time::Duration::ZERO,
                    transaction_ledger_insert_elapsed: std::time::Duration::ZERO,
                    transaction_finalized_delivery_elapsed: std::time::Duration::ZERO,
                    finalization_elapsed: std::time::Duration::from_millis(1),
                    finalization_commit_handlers_elapsed: std::time::Duration::ZERO,
                    finalization_store_commit_elapsed: std::time::Duration::ZERO,
                },
            ))
            .expect("reply import");
    });

    let report = import_chain_acc_from_reader_report(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 0,
            end_height: empty_run as u32,
        }),
        None,
    )
    .await
    .expect("import report");

    service.await.expect("service task");
    assert_eq!(report.imported, (empty_run + 1) as u64);
    assert_eq!(report.empty_blocks, empty_run as u64);
    assert_eq!(report.empty_only_blocks, 0);
    assert!(
        report.empty_block_import_seconds >= 0.02,
        "empty-block elapsed should include the empty-only batch time: {report:?}"
    );
    assert!(
        report.empty_blocks_per_second > 0.0,
        "empty-block BPS should be reported independently from transaction-bearing throughput"
    );
    assert_eq!(report.transaction_blocks, 1);
    assert_eq!(report.transactions, 1);
    assert_eq!(report.transaction_block_import_seconds, 0.001);
    assert!(
        report.transaction_block_import_seconds < report.empty_block_import_seconds,
        "transaction elapsed must exclude empty fast-forward service time: {report:?}"
    );
    assert!(
        (report.transaction_blocks_per_second - 1000.0).abs() < f64::EPSILON,
        "transaction BPS should use transaction-bearing service time: {report:?}"
    );
}

#[tokio::test]
async fn import_chain_acc_uses_fast_forward_sized_batches_for_empty_runs() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let empty_run = neo_blockchain::empty_block_fast_forward::MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS;
    let mut blocks = linked_empty_blocks(0, empty_run);
    let prev = blocks.last().expect("previous block");
    blocks.push(non_empty_block_with_prev_hash(
        empty_run as u32,
        prev.hash(),
        vec![signed_test_transaction(1)],
    ));
    let bytes = encode_chain_acc(&blocks);
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            panic!("expected import blocks command");
        };
        assert_eq!(import.blocks.len(), empty_run + 1);
        assert!(
            import.blocks[..empty_run]
                .iter()
                .all(|block| block.transactions.is_empty())
        );
        assert_eq!(import.blocks[empty_run].transactions.len(), 1);
        reply
            .send(ImportBlocksReply::ok_with_stats(
                import.blocks.len(),
                neo_blockchain::ImportBlocksStats {
                    empty_blocks: empty_run,
                    empty_elapsed: std::time::Duration::from_millis(20),
                    transaction_blocks: 1,
                    transaction_elapsed: std::time::Duration::from_millis(1),
                    transaction_block_clone_elapsed: std::time::Duration::ZERO,
                    transaction_ledger_insert_elapsed: std::time::Duration::ZERO,
                    transaction_finalized_delivery_elapsed: std::time::Duration::ZERO,
                    finalization_elapsed: std::time::Duration::from_millis(1),
                    finalization_commit_handlers_elapsed: std::time::Duration::ZERO,
                    finalization_store_commit_elapsed: std::time::Duration::ZERO,
                },
            ))
            .expect("reply import");
    });

    let report = import_chain_acc_from_reader_report(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 0,
            end_height: empty_run as u32,
        }),
        None,
    )
    .await
    .expect("import report");

    service.await.expect("service task");
    assert_eq!(report.imported, (empty_run + 1) as u64);
    assert_eq!(report.empty_only_blocks, 0);
    assert_eq!(report.transaction_blocks, 1);
}

#[tokio::test]
async fn import_chain_acc_keeps_short_empty_prefix_with_transaction_block() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let empty_run = 24;
    let mut blocks = linked_empty_blocks(0, empty_run);
    let prev = blocks.last().expect("previous block");
    blocks.push(non_empty_block_with_prev_hash(
        empty_run as u32,
        prev.hash(),
        vec![signed_test_transaction(1)],
    ));
    let bytes = encode_chain_acc(&blocks);
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            panic!("expected import blocks command");
        };
        assert_eq!(import.blocks.len(), empty_run + 1);
        assert_eq!(import.blocks[empty_run].transactions.len(), 1);
        reply
            .send(ImportBlocksReply::ok_with_stats(
                import.blocks.len(),
                neo_blockchain::ImportBlocksStats {
                    empty_blocks: empty_run,
                    empty_elapsed: std::time::Duration::from_millis(20),
                    transaction_blocks: 1,
                    transaction_elapsed: std::time::Duration::from_millis(1),
                    transaction_block_clone_elapsed: std::time::Duration::ZERO,
                    transaction_ledger_insert_elapsed: std::time::Duration::ZERO,
                    transaction_finalized_delivery_elapsed: std::time::Duration::ZERO,
                    finalization_elapsed: std::time::Duration::from_millis(1),
                    finalization_commit_handlers_elapsed: std::time::Duration::ZERO,
                    finalization_store_commit_elapsed: std::time::Duration::ZERO,
                },
            ))
            .expect("reply import");
    });

    let report = import_chain_acc_from_reader_report(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 0,
            end_height: empty_run as u32,
        }),
        None,
    )
    .await
    .expect("import report");

    service.await.expect("service task");
    assert_eq!(report.imported, (empty_run + 1) as u64);
    assert_eq!(report.empty_blocks, empty_run as u64);
    assert_eq!(report.empty_only_blocks, 0);
    assert!(
        report.empty_block_import_seconds >= 0.02,
        "short empty-prefix elapsed should come from service-side empty timing: {report:?}"
    );
    assert_eq!(report.transaction_blocks, 1);
    assert_eq!(report.transactions, 1);
    assert!(
        report.transaction_block_import_seconds < report.empty_block_import_seconds,
        "transaction elapsed must exclude short empty-prefix service time: {report:?}"
    );
}

#[tokio::test]
async fn import_chain_acc_keeps_short_empty_suffix_after_transaction_block() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let genesis = empty_block(0);
    let tx = non_empty_block_with_prev_hash(1, genesis.hash(), vec![signed_test_transaction(1)]);
    let mut blocks = vec![genesis, tx];
    for index in 2..26 {
        let prev = blocks.last().expect("previous block");
        blocks.push(empty_block_with_prev_hash(index, prev.hash()));
    }
    let bytes = encode_chain_acc(&blocks);
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            panic!("expected import blocks command");
        };
        assert_eq!(import.blocks.len(), 26);
        assert_eq!(import.blocks[1].transactions.len(), 1);
        reply
            .send(ImportBlocksReply::ok_with_stats(
                import.blocks.len(),
                neo_blockchain::ImportBlocksStats {
                    empty_blocks: 25,
                    empty_elapsed: std::time::Duration::from_millis(20),
                    transaction_blocks: 1,
                    transaction_elapsed: std::time::Duration::from_millis(1),
                    transaction_block_clone_elapsed: std::time::Duration::ZERO,
                    transaction_ledger_insert_elapsed: std::time::Duration::ZERO,
                    transaction_finalized_delivery_elapsed: std::time::Duration::ZERO,
                    finalization_elapsed: std::time::Duration::from_millis(1),
                    finalization_commit_handlers_elapsed: std::time::Duration::ZERO,
                    finalization_store_commit_elapsed: std::time::Duration::ZERO,
                },
            ))
            .expect("reply import");
    });

    let report = import_chain_acc_from_reader_report(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 0,
            end_height: 25,
        }),
        None,
    )
    .await
    .expect("import report");

    service.await.expect("service task");
    assert_eq!(report.imported, 26);
    assert_eq!(report.empty_blocks, 25);
    assert_eq!(report.empty_only_blocks, 0);
    assert!(
        report.empty_block_import_seconds >= 0.02,
        "short empty suffix elapsed should come from service-side empty timing: {report:?}"
    );
    assert_eq!(report.transaction_blocks, 1);
    assert_eq!(report.transactions, 1);
    assert!(
        report.transaction_block_import_seconds < report.empty_block_import_seconds,
        "transaction elapsed must exclude short empty-suffix service time: {report:?}"
    );
}

#[tokio::test]
async fn import_chain_acc_uses_service_timing_without_splitting_mixed_batches() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let genesis = empty_block(0);
    let tx = non_empty_block_with_prev_hash(1, genesis.hash(), vec![signed_test_transaction(1)]);
    let mut blocks = vec![genesis, tx];
    for index in 2..26 {
        let prev = blocks.last().expect("previous block");
        blocks.push(empty_block_with_prev_hash(index, prev.hash()));
    }
    let bytes = encode_chain_acc(&blocks);
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            panic!("expected import blocks command");
        };
        assert_eq!(
            import.blocks.len(),
            26,
            "short mixed runs should stay in one bulk import command"
        );
        reply
            .send(ImportBlocksReply::ok_with_stats(
                import.blocks.len(),
                neo_blockchain::ImportBlocksStats {
                    empty_blocks: 25,
                    empty_elapsed: std::time::Duration::from_millis(20),
                    transaction_blocks: 1,
                    transaction_elapsed: std::time::Duration::from_millis(5),
                    transaction_block_clone_elapsed: std::time::Duration::ZERO,
                    transaction_ledger_insert_elapsed: std::time::Duration::ZERO,
                    transaction_finalized_delivery_elapsed: std::time::Duration::ZERO,
                    finalization_elapsed: std::time::Duration::from_millis(2),
                    finalization_commit_handlers_elapsed: std::time::Duration::ZERO,
                    finalization_store_commit_elapsed: std::time::Duration::ZERO,
                },
            ))
            .expect("reply import");
    });

    let report = import_chain_acc_from_reader_report(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 0,
            end_height: 25,
        }),
        None,
    )
    .await
    .expect("import report");

    service.await.expect("service task");
    assert_eq!(report.imported, 26);
    assert_eq!(report.empty_blocks, 25);
    assert_eq!(report.empty_only_blocks, 0);
    assert_eq!(report.transaction_blocks, 1);
    assert_eq!(report.transactions, 1);
    assert!(
        report.empty_block_import_seconds >= 0.02,
        "empty elapsed should come from service-side fast-forward timing: {report:?}"
    );
    assert!(
        report.transaction_block_import_seconds >= 0.005,
        "transaction elapsed should come from service-side transaction timing: {report:?}"
    );
    assert!(
        report.transaction_block_import_seconds < report.empty_block_import_seconds,
        "service timing must let transaction proof exclude empty fast-forward time: {report:?}"
    );
}

#[test]
fn chain_acc_batch_keeps_short_mixed_runs_until_normal_boundary() {
    let empty_limit = neo_blockchain::empty_block_fast_forward::MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS;
    let mut small_empty_prefix = linked_empty_blocks(0, empty_limit - 1);
    let mut pending = PendingChainAccBatch::default();
    for block in &small_empty_prefix {
        pending.record_pushed(block);
    }
    let prev = small_empty_prefix.last().expect("previous block");
    let next = non_empty_block_with_prev_hash(
        (empty_limit - 1) as u32,
        prev.hash(),
        vec![signed_test_transaction(1)],
    );
    assert!(
        !pending.should_flush(small_empty_prefix.len()),
        "short empty prefixes should stay in the mixed bulk import; service-side stats separate their timing"
    );
    pending.record_pushed(&next);
    small_empty_prefix.push(next);
    assert!(
        !pending.should_flush(small_empty_prefix.len()),
        "transaction runs still wait for the normal import boundary until the next empty block"
    );
    let following_empty = empty_block_with_prev_hash(
        small_empty_prefix.len() as u32,
        small_empty_prefix.last().expect("previous block").hash(),
    );
    pending.record_pushed(&following_empty);
    small_empty_prefix.push(following_empty);
    assert!(
        !pending.should_flush(small_empty_prefix.len()),
        "short empty suffixes should not force an extra bulk finalization boundary"
    );

    let empty_run = empty_limit;
    let mut blocks = linked_empty_blocks(0, empty_run);
    let mut pending = PendingChainAccBatch::default();
    for block in &blocks {
        pending.record_pushed(block);
    }
    assert!(
        !pending.should_flush(blocks.len()),
        "empty-only outer chain.acc batches do not flush at the service-internal fast-forward chunk size"
    );
    let prev = blocks.last().expect("previous block");
    blocks.push(non_empty_block_with_prev_hash(
        empty_run as u32,
        prev.hash(),
        vec![signed_test_transaction(1)],
    ));
    pending.record_pushed(blocks.last().expect("transaction block"));

    assert!(
        !pending.should_flush(blocks.len()),
        "transaction blocks can share the outer batch with a fast-forwardable empty prefix"
    );
}

#[test]
fn pending_chain_acc_batch_tracks_transaction_presence_without_scanning_blocks() {
    let mut pending = PendingChainAccBatch::default();
    let empty = empty_block(0);
    pending.record_pushed(&empty);

    assert!(pending_batch_is_empty_only(&pending));
    assert!(!pending.should_flush(1));

    let tx = non_empty_block_with_prev_hash(1, empty.hash(), vec![signed_test_transaction(1)]);
    pending.record_pushed(&tx);

    assert!(!pending_batch_is_empty_only(&pending));
    assert!(!pending.should_flush(2));
    for index in 2..IMPORT_BATCH_SIZE {
        pending.record_pushed(&empty_block(index as u32));
    }
    assert!(!pending_batch_is_empty_only(&pending));
    assert!(pending.should_flush(IMPORT_BATCH_SIZE));
}

#[test]
fn empty_only_chain_acc_batches_flush_at_outer_import_boundary() {
    let mut pending = PendingChainAccBatch::default();
    let empty = empty_block(0);

    let max = neo_blockchain::empty_block_fast_forward::MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS;
    for _ in 0..max {
        pending.record_pushed(&empty);
    }

    assert!(
        !pending.should_flush(max),
        "chain.acc owns only the outer import boundary; the blockchain service chunks empty runs internally"
    );
    for _ in max..IMPORT_BATCH_SIZE {
        pending.record_pushed(&empty);
    }
    assert!(pending.should_flush(IMPORT_BATCH_SIZE));
}

#[test]
fn chain_acc_batch_import_uses_tracked_composition_without_rescanning_blocks() {
    let source = include_str!("../../../node/chain_acc/batch.rs");
    let batch_import = source
        .split("async fn import_chain_acc_batch")
        .nth(1)
        .and_then(|tail| {
            tail.split("#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]")
                .next()
        })
        .expect("import_chain_acc_batch source");

    assert!(
        !batch_import.contains("ChainAccImportComposition::from_blocks(&batch_blocks)"),
        "chain.acc import should reuse composition tracked while reading, not rescan every batch before dispatch"
    );
}

#[test]
fn pending_chain_acc_batch_derives_transaction_presence_from_composition() {
    let source = include_str!("../../../node/chain_acc/batch.rs");
    let pending_batch = source
        .split("struct PendingChainAccBatch")
        .nth(1)
        .and_then(|tail| tail.split("struct ChainAccBatchImportResult").next())
        .expect("PendingChainAccBatch source");

    assert!(
        !pending_batch.contains("has_transactions"),
        "pending chain.acc batch should not duplicate transaction-presence state once composition is tracked"
    );
}

#[test]
fn chain_acc_batch_import_uses_tracked_tip_without_rehashing_last_block() {
    let source = include_str!("../../../node/chain_acc/batch.rs");
    let batch_import = source
        .split("async fn import_chain_acc_batch")
        .nth(1)
        .and_then(|tail| {
            tail.split("#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]")
                .next()
        })
        .expect("import_chain_acc_batch source");

    assert!(
        !batch_import.contains("batch_blocks.last().map"),
        "chain.acc import should reuse the tip tracked while reading, not rehash the last block before dispatch"
    );
}

#[tokio::test]
async fn import_chain_acc_report_uses_zero_bps_for_noop_resume() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let blocks = linked_empty_blocks(0, 3);
    let local_tip_hash = blocks[2].hash();
    let store = memory_store_with_ledger_tip(2, local_tip_hash);
    let bytes = encode_chain_acc(&blocks);
    let mut cursor = std::io::Cursor::new(bytes);

    let report = import_chain_acc_from_reader_report(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 0,
            end_height: 2,
        }),
        Some(store),
    )
    .await
    .expect("noop report");

    assert!(
        commands.try_recv().is_err(),
        "noop resume should not import"
    );
    assert_eq!(report.imported, 0);
    assert_eq!(report.average_blocks_per_second, 0.0);
}

#[tokio::test]
async fn import_chain_acc_rejects_partial_range_without_storage_before_import() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let bytes = encode_chain_acc(&[empty_block(10)]);
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            return false;
        };
        let imported = import.blocks.len();
        reply
            .send(ImportBlocksReply::ok(imported))
            .expect("reply import");
        true
    });

    let result = import_chain_acc_from_reader(
        &handle,
        &mut cursor,
        None,
        false,
        Some(ChainAccExpectedRange {
            start_height: 10,
            end_height: 10,
        }),
        None,
    )
    .await;
    service.abort();

    assert!(
        result.is_err(),
        "partial expected-range import without storage must fail, got {result:?}"
    );
    let err =
        result.expect_err("partial expected-range import needs storage for continuity validation");
    assert!(
        err.to_string().contains("requires local storage"),
        "unexpected error: {err}"
    );
}
