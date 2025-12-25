use super::helpers::PersistCompletedHarness;
use neo_primitives::UInt256;

#[tokio::test]
async fn persist_completed_starts_consensus_round() {
    let network = 0x4E454F;
    let mut harness = PersistCompletedHarness::new(network, 4);
    let prev_hash = UInt256::from([0x01; 32]);

    harness
        .persist_completed_all(0, prev_hash, 1_000)
        .expect("persist completed");
    harness
        .drive_until_idle(50)
        .await
        .expect("drive");

    assert!(harness.saw_prepare_request(1));
}

#[tokio::test]
async fn persist_completed_multiple_rounds() {
    let network = 0x4E454F;
    let mut harness = PersistCompletedHarness::new(network, 4);

    for round in 0u32..3 {
        let prev_hash = UInt256::from([round as u8; 32]);
        harness
            .persist_completed_all(round, prev_hash, 1_000 + round as u64)
            .expect("persist completed");
        harness
            .drive_until_idle(50)
            .await
            .expect("drive");

        assert!(harness.saw_prepare_request(round + 1));
        harness.take_events();
    }
}

#[tokio::test]
async fn persist_completed_round_emits_block_committed() {
    let network = 0x4E454F;
    let mut harness = PersistCompletedHarness::new(network, 4);
    let prev_hash = UInt256::from([0x02; 32]);

    harness
        .persist_completed_all(0, prev_hash, 1_000)
        .expect("persist completed");
    harness
        .drive_until_idle(200)
        .await
        .expect("drive");

    assert!(harness.saw_block_committed(1));
}
