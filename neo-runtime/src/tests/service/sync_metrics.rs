use super::*;

#[test]
fn native_persist_tx_stage_tracks_exact_cumulative_time_and_ewma() {
    let before = native_persist_tx_stage_stats()
        .into_iter()
        .find(|stat| stat.stage == "container_prepare")
        .expect("container_prepare stage");

    record_native_persist_tx_stage(NativePersistTxStage::ContainerPrepare, 160);
    record_native_persist_tx_stage(NativePersistTxStage::ContainerPrepare, 320);

    let after = native_persist_tx_stage_stats()
        .into_iter()
        .find(|stat| stat.stage == "container_prepare")
        .expect("container_prepare stage");
    let first_ewma = if before.avg_us == 0 {
        160
    } else {
        (before.avg_us as i64 + (160_i64 - before.avg_us as i64) / 16).max(0) as u64
    };
    let expected_ewma = (first_ewma as i64 + (320_i64 - first_ewma as i64) / 16).max(0) as u64;

    assert_eq!(after.calls.saturating_sub(before.calls), 2);
    assert_eq!(after.total_us.saturating_sub(before.total_us), 480);
    assert_eq!(after.avg_us, expected_ewma);

    let all = native_persist_tx_stage_stats();
    let hot = native_persist_tx_hot_stats().expect("hot native transaction stage");
    let matching = all
        .into_iter()
        .find(|stat| stat.stage == hot.stage)
        .expect("hot stage in full snapshot");
    assert_eq!(hot, matching);
}
