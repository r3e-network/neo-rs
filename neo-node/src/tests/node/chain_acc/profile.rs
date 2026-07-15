use super::*;

#[tokio::test]
async fn import_report_retains_batch_profile_window() {
    let (handle, mut commands, _events) = BlockchainHandle::channel(1, 1);
    let genesis = empty_block(0);
    let block1 =
        non_empty_block_with_prev_hash(1, genesis.hash(), vec![signed_test_transaction(1)]);
    let block2 = empty_block_with_prev_hash(2, block1.hash());
    let bytes = encode_chain_acc(&[genesis, block1, block2]);
    let mut cursor = std::io::Cursor::new(bytes);
    let service = tokio::spawn(async move {
        let Some(BlockchainCommand::ImportBlocks { import, reply }) = commands.recv().await else {
            panic!("expected import blocks command");
        };
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
    assert_eq!(report.transaction_block_clone_seconds, 0.003);
    assert_eq!(report.transaction_ledger_insert_seconds, 0.004);
    assert_eq!(report.transaction_finalized_delivery_seconds, 0.005);
    assert_eq!(report.profile_windows.len(), 1);
    let window = &report.profile_windows[0];
    assert_eq!(
        (window.start_height, window.end_height, window.blocks),
        (0, 2, 3)
    );
    assert_eq!(window.empty_blocks, 2);
    assert_eq!(window.empty_block_import_seconds, 0.002);
    assert_eq!(window.empty_blocks_per_second, 1000.0);
    assert_eq!(window.transaction_blocks, 1);
    assert_eq!(window.transactions, 1);
    assert_eq!(window.transaction_block_import_seconds, 0.001);
    assert_eq!(window.transaction_blocks_per_second, 1000.0);
    assert_eq!(window.finalization_seconds, 0.001);
    assert_eq!(window.finalization_commit_handlers_seconds, 0.0006);
    assert_eq!(window.finalization_canonical_commit_seconds, 0.0004);
}
