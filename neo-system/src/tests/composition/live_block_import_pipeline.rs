use super::*;

fn empty_block(index: u32) -> Arc<Block> {
    let mut header = neo_payloads::Header::new();
    header.set_index(index);
    Arc::new(Block::from_parts(header, Vec::new()))
}

#[tokio::test]
async fn live_pipeline_filters_bad_candidates_and_preserves_arc_order() {
    let (blockchain, mut commands, _events) = BlockchainHandle::channel(4, 4);
    let queue = Arc::new(BlockImportQueue::new(Arc::new(blockchain.clone()), 2));
    let pipeline = LiveBlockImportPipeline::new(blockchain, Arc::clone(&queue));
    let first = empty_block(1);
    let mut malformed = Block::new();
    malformed.header.set_index(2);
    malformed
        .header
        .set_merkle_root(neo_primitives::UInt256::from([0x42; 32]));
    let malformed = Arc::new(malformed);
    let third = empty_block(3);

    let summary = pipeline
        .submit_peer_blocks(vec![Arc::clone(&first), malformed, Arc::clone(&third)])
        .await
        .expect("submit checked live burst");

    assert_eq!(
        summary,
        LiveBlockImportSummary {
            received: 3,
            submitted: 2,
            rejected: 1,
        }
    );
    assert!(Arc::ptr_eq(&pipeline.import_queue(), &queue));
    match commands.recv().await.expect("checked inventory command") {
        neo_blockchain::BlockchainCommand::CheckedInventoryBlocks { checked, relay } => {
            assert!(relay);
            assert_eq!(checked.rejected_len(), 1);
            assert_eq!(checked.rejected()[0].position(), 1);
            assert!(Arc::ptr_eq(&checked.blocks()[0], &first));
            assert!(Arc::ptr_eq(&checked.blocks()[1], &third));
        }
        other => panic!("expected checked inventory batch, got {other:?}"),
    }
}

#[tokio::test]
async fn live_pipeline_does_not_enqueue_an_all_rejected_batch() {
    let (blockchain, mut commands, _events) = BlockchainHandle::channel(4, 4);
    let queue = Arc::new(BlockImportQueue::new(Arc::new(blockchain.clone()), 1));
    let pipeline = LiveBlockImportPipeline::new(blockchain, queue);
    let mut malformed = Block::new();
    malformed
        .header
        .set_merkle_root(neo_primitives::UInt256::from([0x42; 32]));

    let summary = pipeline
        .submit_peer_blocks(vec![Arc::new(malformed)])
        .await
        .expect("reject malformed burst");

    assert_eq!(summary.submitted, 0);
    assert_eq!(summary.rejected, 1);
    assert!(commands.try_recv().is_err());
}
