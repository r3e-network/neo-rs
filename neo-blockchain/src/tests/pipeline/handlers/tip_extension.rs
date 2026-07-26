use super::*;
use neo_payloads::header::Header;

/// A consensus block sits at the next expected height yet descends from the
/// wrong parent. `consensus_witness_verified = true` disables the only other
/// check that inspects the header's authenticity, so without an explicit parent
/// guard this persists an unverifiable header and forks the node.
///
/// This is the dBFT prev_hash fork: the driver assembled the committed block
/// from its speculatively-advanced `prev_hash` instead of the round's agreed
/// parent, and the node stayed off a private network for ~1800 blocks.
#[tokio::test]
async fn consensus_block_at_next_height_with_wrong_parent_is_rejected() {
    let (service, _handle, snapshot) = store_fixture();
    service.initialize().await.expect("initialize");

    let settings = neo_config::ProtocolSettings::default();
    let genesis =
        crate::native_persist::genesis_block(&chain_spec_for_settings(&settings)).expect("genesis");

    // Height 1 is exactly `current_height + 1`, and the witness check is
    // bypassed, so only the parent link can reject this block.
    let mut forked = Header::new();
    forked.set_index(1);
    forked.set_prev_hash(neo_primitives::UInt256::zero());
    forked.set_timestamp(genesis.header.timestamp() + 15_000);
    forked.set_next_consensus(*genesis.header.next_consensus());
    let forked_block = Arc::new(Block::from_parts(forked, vec![]));

    let error = service
        .handle_block_inventory(Arc::clone(&forked_block), false, true)
        .await
        .expect_err("a consensus block that does not extend the tip must be rejected");
    let message = error.to_string();
    assert!(
        message.contains("does not extend current tip"),
        "expected a parent-link rejection, got: {message}"
    );

    // The tip must not have moved and nothing may have been parked: the block
    // was contiguous by height, so it is invalid rather than early.
    assert_eq!(service.ledger.current_height(), 0);
    assert_eq!(service.unverified_block_count(), 0);
    assert!(service.ledger.block_hash_at(1).is_none());
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        0
    );

    // The correctly parented block at the same height still persists, proving
    // the guard rejects the wrong parent rather than the height.
    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());

    service
        .handle_block_inventory(Arc::new(Block::from_parts(header1, vec![])), false, true)
        .await
        .expect("the block that extends the tip persists");
    assert_eq!(service.ledger.current_height(), 1);
}
