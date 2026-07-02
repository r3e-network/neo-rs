use super::*;

use std::sync::{Arc, Mutex, MutexGuard};

use neo_config::{Hardfork, ProtocolSettings};
use neo_payloads::{Block, Header, Transaction};
use neo_storage::SeekDirection;

use crate::empty_block_fast_forward::EmptyBlockFastForwardRequest;
use crate::native_persist::{NativePersistOptions, NativePersistResources};
use crate::service_context::BlockPersistContext;

static PROVIDER_TEST_LOCK: Mutex<()> = Mutex::new(());

fn lock_provider() -> MutexGuard<'static, ()> {
    PROVIDER_TEST_LOCK.lock().expect("provider test lock")
}

fn install_resources() -> NativePersistResources {
    neo_native_contracts::install();
    NativePersistResources::from_installed_provider().expect("native resources")
}

fn empty_block(index: u32) -> Arc<Block> {
    let mut header = Header::new();
    header.set_index(index);
    Arc::new(Block::from_parts(header, Vec::new()))
}

fn empty_child(prev: &Block, index: u32) -> Arc<Block> {
    let mut header = Header::new();
    header.set_index(index);
    header.set_prev_hash(prev.hash());
    header.set_timestamp(prev.header.timestamp() + 15_000);
    header.set_next_consensus(*prev.header.next_consensus());
    Arc::new(Block::from_parts(header, Vec::new()))
}

fn non_empty_block(index: u32) -> Arc<Block> {
    let mut header = Header::new();
    header.set_index(index);
    Arc::new(Block::from_parts(header, vec![Transaction::new()]))
}

fn empty_block_with_merkle_root(index: u32, byte: u8) -> Arc<Block> {
    let mut header = Header::new();
    header.set_index(index);
    header.set_merkle_root(neo_primitives::UInt256::from([byte; 32]));
    Arc::new(Block::from_parts(header, Vec::new()))
}

fn bulk_options() -> NativePersistOptions {
    NativePersistOptions {
        capture_replay_artifacts: false,
    }
}

fn bulk_context() -> BlockPersistContext {
    BlockPersistContext::bulk_sync()
}

fn visible_store_dump(snapshot: &neo_storage::DataCache) -> Vec<(Vec<u8>, Vec<u8>)> {
    snapshot
        .find(None, SeekDirection::Forward)
        .map(|(key, item)| (key.to_array(), item.value_bytes().into_owned()))
        .collect()
}

fn neo_gas_per_block_key(index: u32) -> neo_storage::StorageKey {
    let mut key = vec![29];
    key.extend_from_slice(&index.to_be_bytes());
    neo_storage::StorageKey::new(neo_native_contracts::NeoToken::ID, key)
}

#[test]
fn planner_accepts_contiguous_empty_bulk_sync_range_between_cut_points() {
    let _guard = lock_provider();
    let resources = install_resources();
    let settings = ProtocolSettings::default();
    let blocks = vec![empty_block(1), empty_block(2), empty_block(3)];

    let plan = plan_empty_block_fast_forward(EmptyBlockFastForwardRequest {
        current_height: 0,
        blocks: &blocks,
        settings: &settings,
        resources: &resources,
        persist_options: bulk_options(),
        persist_context: bulk_context(),
    })
    .expect("empty bulk-sync range should be eligible");

    assert_eq!(
        plan,
        EmptyBlockFastForwardPlan {
            start: 1,
            end: 3,
            block_count: 3,
        }
    );
}

#[test]
fn planner_accepts_maximum_empty_blocks_as_one_batch() {
    let _guard = lock_provider();
    let resources = install_resources();
    let settings = ProtocolSettings::default();
    let count = crate::empty_block_fast_forward::MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS;
    let blocks = (1..=count as u32).map(empty_block).collect::<Vec<_>>();

    let plan = plan_empty_block_fast_forward(EmptyBlockFastForwardRequest {
        current_height: 0,
        blocks: &blocks,
        settings: &settings,
        resources: &resources,
        persist_options: bulk_options(),
        persist_context: bulk_context(),
    })
    .expect("maximum empty bulk-sync range should be one eligible fast-forward batch");

    assert_eq!(
        plan,
        EmptyBlockFastForwardPlan {
            start: 1,
            end: count as u32,
            block_count: count,
        }
    );
}

#[test]
fn planner_batch_guard_matches_realistic_short_empty_runs() {
    assert_eq!(
        crate::empty_block_fast_forward::MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS,
        128,
        "empty-block fast-forward should publish staged writes in short bounded bursts"
    );
}

#[test]
fn planner_rejects_only_above_the_empty_block_batch_guard() {
    let _guard = lock_provider();
    let resources = install_resources();
    let settings = ProtocolSettings::default();
    let count = crate::empty_block_fast_forward::MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS + 1;
    let blocks = (1..=count as u32).map(empty_block).collect::<Vec<_>>();

    let err = plan_empty_block_fast_forward(EmptyBlockFastForwardRequest {
        current_height: 0,
        blocks: &blocks,
        settings: &settings,
        resources: &resources,
        persist_options: bulk_options(),
        persist_context: bulk_context(),
    })
    .expect_err("only the memory/fairness guard should cap one internal chunk");

    assert_eq!(
        err,
        EmptyBlockFastForwardRejection::BatchTooLarge {
            count,
            max: crate::empty_block_fast_forward::MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS,
        }
    );
}

#[test]
fn planner_rejects_replay_artifact_paths() {
    let _guard = lock_provider();
    let resources = install_resources();
    let settings = ProtocolSettings::default();
    let blocks = vec![empty_block(1)];

    let err = plan_empty_block_fast_forward(EmptyBlockFastForwardRequest {
        current_height: 0,
        blocks: &blocks,
        settings: &settings,
        resources: &resources,
        persist_options: NativePersistOptions::default(),
        persist_context: bulk_context(),
    })
    .expect_err("event/replay paths must stay on normal per-block persistence");

    assert_eq!(err, EmptyBlockFastForwardRejection::ReplayArtifactsEnabled);
}

#[test]
fn planner_rejects_non_bulk_sync_contexts() {
    let _guard = lock_provider();
    let resources = install_resources();
    let settings = ProtocolSettings::default();
    let blocks = vec![empty_block(1)];

    let err = plan_empty_block_fast_forward(EmptyBlockFastForwardRequest {
        current_height: 0,
        blocks: &blocks,
        settings: &settings,
        resources: &resources,
        persist_options: bulk_options(),
        persist_context: BlockPersistContext::live(),
    })
    .expect_err("live import must preserve per-block persistence semantics");

    assert_eq!(err, EmptyBlockFastForwardRejection::NotBulkSync);
}

#[test]
fn planner_rejects_blocks_with_transactions() {
    let _guard = lock_provider();
    let resources = install_resources();
    let settings = ProtocolSettings::default();
    let blocks = vec![empty_block(1), non_empty_block(2)];

    let err = plan_empty_block_fast_forward(EmptyBlockFastForwardRequest {
        current_height: 0,
        blocks: &blocks,
        settings: &settings,
        resources: &resources,
        persist_options: bulk_options(),
        persist_context: bulk_context(),
    })
    .expect_err("transaction-bearing blocks must prove execution speed normally");

    assert_eq!(
        err,
        EmptyBlockFastForwardRejection::ContainsTransactions {
            height: 2,
            tx_count: 1,
        }
    );
}

#[test]
fn planner_rejects_non_next_start() {
    let _guard = lock_provider();
    let resources = install_resources();
    let settings = ProtocolSettings::default();
    let blocks = vec![empty_block(2)];

    let err = plan_empty_block_fast_forward(EmptyBlockFastForwardRequest {
        current_height: 0,
        blocks: &blocks,
        settings: &settings,
        resources: &resources,
        persist_options: bulk_options(),
        persist_context: bulk_context(),
    })
    .expect_err("fast-forward ranges must begin at CurrentIndex + 1");

    assert_eq!(
        err,
        EmptyBlockFastForwardRejection::NonNextStart {
            expected: 1,
            actual: 2,
        }
    );
}

#[test]
fn planner_rejects_non_contiguous_range() {
    let _guard = lock_provider();
    let resources = install_resources();
    let settings = ProtocolSettings::default();
    let blocks = vec![empty_block(1), empty_block(3)];

    let err = plan_empty_block_fast_forward(EmptyBlockFastForwardRequest {
        current_height: 0,
        blocks: &blocks,
        settings: &settings,
        resources: &resources,
        persist_options: bulk_options(),
        persist_context: bulk_context(),
    })
    .expect_err("fast-forward ranges must be contiguous");

    assert_eq!(
        err,
        EmptyBlockFastForwardRejection::NonContiguous {
            expected: 2,
            actual: 3,
        }
    );
}

#[test]
fn planner_rejects_empty_blocks_with_non_zero_merkle_root() {
    let _guard = lock_provider();
    let resources = install_resources();
    let settings = ProtocolSettings::default();
    let merkle_root = neo_primitives::UInt256::from([0x42; 32]);
    let blocks = vec![empty_block(1), empty_block_with_merkle_root(2, 0x42)];

    let err = plan_empty_block_fast_forward(EmptyBlockFastForwardRequest {
        current_height: 0,
        blocks: &blocks,
        settings: &settings,
        resources: &resources,
        persist_options: bulk_options(),
        persist_context: bulk_context(),
    })
    .expect_err("empty fast-forward requires C# empty-block merkle roots");

    assert_eq!(
        err,
        EmptyBlockFastForwardRejection::NonEmptyMerkleRoot {
            height: 2,
            merkle_root,
        }
    );
}

#[test]
fn planner_rejects_native_initialization_or_manifest_refresh_height() {
    let _guard = lock_provider();
    let resources = install_resources();
    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfEchidna, 2);
    let blocks = vec![empty_block(1), empty_block(2)];

    let err = plan_empty_block_fast_forward(EmptyBlockFastForwardRequest {
        current_height: 0,
        blocks: &blocks,
        settings: &settings,
        resources: &resources,
        persist_options: bulk_options(),
        persist_context: bulk_context(),
    })
    .expect_err("native initialization cut points must fall back to normal persist");

    assert!(
        matches!(
            err,
            EmptyBlockFastForwardRejection::NativeInitializationHeight { height: 2, .. }
        ),
        "unexpected rejection: {err}"
    );
}

#[test]
fn standard_natives_explicitly_opt_in_to_empty_block_fast_forward() {
    let _guard = lock_provider();
    let resources = install_resources();
    let names = resources
        .contracts()
        .iter()
        .map(|contract| {
            (
                contract.name().to_string(),
                contract.supports_empty_block_fast_forward(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        names.len(),
        neo_native_contracts::STANDARD_NATIVE_CONTRACT_COUNT
    );
    assert!(
        names.iter().all(|(_, supported)| *supported),
        "all active standard natives must explicitly opt in: {names:?}"
    );
}

#[test]
fn stage_empty_block_fast_forward_reuses_last_hash_from_ledger_loop() {
    let source = include_str!("../../pipeline/empty_block_fast_forward.rs");
    let stage = source
        .split("pub fn stage_empty_block_fast_forward")
        .nth(1)
        .and_then(|tail| tail.split("#[cfg(test)]").next())
        .expect("stage_empty_block_fast_forward source");

    assert_eq!(
        stage.matches(".try_hash()").count(),
        1,
        "empty-block fast-forward should not hash the final block twice"
    );
}

#[test]
fn fast_forward_empty_blocks_matches_normal_persist_store_dump_between_refreshes() {
    let _guard = lock_provider();
    let resources = install_resources();
    let settings = ProtocolSettings::default();

    let normal = Arc::new(neo_storage::DataCache::new(false));
    let fast = Arc::new(neo_storage::DataCache::new(false));
    let genesis = Arc::new(crate::native_persist::genesis_block(&settings).expect("genesis"));
    let block1 = empty_child(&genesis, 1);
    let block2 = empty_child(&block1, 2);
    let block3 = empty_child(&block2, 3);
    let blocks = vec![
        Arc::clone(&block1),
        Arc::clone(&block2),
        Arc::clone(&block3),
    ];

    crate::native_persist::persist_block_natives(
        Arc::clone(&normal),
        Arc::clone(&genesis),
        &settings,
    )
    .expect("normal genesis");
    crate::native_persist::persist_block_natives(Arc::clone(&fast), genesis, &settings)
        .expect("fast genesis baseline");

    // A governance change from an earlier non-empty block can schedule a new
    // gasPerBlock record effective inside an otherwise empty run. The fast path
    // must keep the normal single-block integer-division boundary for each
    // height before aggregating rewards.
    let gas_change_key = neo_gas_per_block_key(3);
    let gas_change_value = neo_storage::StorageItem::from_bytes(
        num_bigint::BigInt::from(7 * 100_000_000i64).to_signed_bytes_le(),
    );
    normal.update(gas_change_key.clone(), gas_change_value.clone());
    fast.update(gas_change_key, gas_change_value);

    for block in &blocks {
        crate::native_persist::persist_block_natives(
            Arc::clone(&normal),
            Arc::clone(block),
            &settings,
        )
        .expect("normal empty block");
    }

    let staged = stage_empty_block_fast_forward(
        Arc::clone(&fast),
        &blocks,
        &settings,
        bulk_options(),
        bulk_context(),
        &resources,
        0,
    )
    .expect("fast-forward stage");
    staged.commit();

    assert_eq!(
        visible_store_dump(&fast),
        visible_store_dump(&normal),
        "fast-forward must be byte-equivalent to normal native persistence for the covered run"
    );

    for block in blocks {
        let hash = block.hash();
        assert!(
            neo_native_contracts::LedgerContract::new()
                .get_trimmed_block(&fast, &hash)
                .expect("ledger trimmed block lookup")
                .is_some(),
            "ledger history must retain empty block {}",
            block.index()
        );
    }
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&fast)
            .expect("current index"),
        3
    );
}

#[test]
fn fast_forward_empty_blocks_matches_normal_persist_across_multiple_gas_changes() {
    let _guard = lock_provider();
    let resources = install_resources();
    let settings = ProtocolSettings::default();

    let normal = Arc::new(neo_storage::DataCache::new(false));
    let fast = Arc::new(neo_storage::DataCache::new(false));
    let genesis = Arc::new(crate::native_persist::genesis_block(&settings).expect("genesis"));
    crate::native_persist::persist_block_natives(
        Arc::clone(&normal),
        Arc::clone(&genesis),
        &settings,
    )
    .expect("normal genesis");
    crate::native_persist::persist_block_natives(
        Arc::clone(&fast),
        Arc::clone(&genesis),
        &settings,
    )
    .expect("fast genesis baseline");

    for (index, gas_per_block) in [(3, 7), (5, 11), (7, 13)] {
        let key = neo_gas_per_block_key(index);
        let value = neo_storage::StorageItem::from_bytes(
            num_bigint::BigInt::from(gas_per_block * 100_000_000i64).to_signed_bytes_le(),
        );
        normal.update(key.clone(), value.clone());
        fast.update(key, value);
    }

    let mut prev = genesis;
    let mut blocks = Vec::new();
    for height in 1..=8 {
        let block = empty_child(&prev, height);
        prev = Arc::clone(&block);
        blocks.push(block);
    }

    for block in &blocks {
        crate::native_persist::persist_block_natives(
            Arc::clone(&normal),
            Arc::clone(block),
            &settings,
        )
        .expect("normal empty block across gas changes");
    }

    let staged = stage_empty_block_fast_forward(
        Arc::clone(&fast),
        &blocks,
        &settings,
        bulk_options(),
        bulk_context(),
        &resources,
        0,
    )
    .expect("fast-forward across multiple gas changes");
    staged.commit();

    assert_eq!(
        visible_store_dump(&fast),
        visible_store_dump(&normal),
        "fast-forward must be byte-equivalent across multiple gas-per-block records"
    );
}

#[test]
fn fast_forward_empty_blocks_matches_normal_persist_across_committee_refresh() {
    let _guard = lock_provider();
    let resources = install_resources();
    let settings = ProtocolSettings::default();
    let committee_count = settings.committee_members_count() as u32;

    let normal = Arc::new(neo_storage::DataCache::new(false));
    let fast = Arc::new(neo_storage::DataCache::new(false));
    let genesis = Arc::new(crate::native_persist::genesis_block(&settings).expect("genesis"));
    crate::native_persist::persist_block_natives(
        Arc::clone(&normal),
        Arc::clone(&genesis),
        &settings,
    )
    .expect("normal genesis");
    crate::native_persist::persist_block_natives(
        Arc::clone(&fast),
        Arc::clone(&genesis),
        &settings,
    )
    .expect("fast genesis baseline");

    let mut prev = genesis;
    for height in 1..committee_count - 1 {
        prev = empty_child(&prev, height);
        crate::native_persist::persist_block_natives(
            Arc::clone(&normal),
            Arc::clone(&prev),
            &settings,
        )
        .expect("normal pre-run block");
        crate::native_persist::persist_block_natives(
            Arc::clone(&fast),
            Arc::clone(&prev),
            &settings,
        )
        .expect("fast pre-run block");
    }

    let block_before_refresh = empty_child(&prev, committee_count - 1);
    let refresh_block = empty_child(&block_before_refresh, committee_count);
    let block_after_refresh = empty_child(&refresh_block, committee_count + 1);
    let blocks = vec![
        Arc::clone(&block_before_refresh),
        Arc::clone(&refresh_block),
        Arc::clone(&block_after_refresh),
    ];

    for block in &blocks {
        crate::native_persist::persist_block_natives(
            Arc::clone(&normal),
            Arc::clone(block),
            &settings,
        )
        .expect("normal empty block across refresh");
    }

    let staged = stage_empty_block_fast_forward(
        Arc::clone(&fast),
        &blocks,
        &settings,
        bulk_options(),
        bulk_context(),
        &resources,
        committee_count - 2,
    )
    .expect("fast-forward across committee refresh");
    staged.commit();

    assert_eq!(
        visible_store_dump(&fast),
        visible_store_dump(&normal),
        "fast-forward must be byte-equivalent across a committee refresh"
    );
}

#[test]
fn fast_forward_empty_blocks_matches_normal_persist_across_multiple_committee_refreshes() {
    let _guard = lock_provider();
    let resources = install_resources();
    let settings = ProtocolSettings::default();
    let committee_count = settings.committee_members_count() as u32;

    let normal = Arc::new(neo_storage::DataCache::new(false));
    let fast = Arc::new(neo_storage::DataCache::new(false));
    let genesis = Arc::new(crate::native_persist::genesis_block(&settings).expect("genesis"));
    crate::native_persist::persist_block_natives(
        Arc::clone(&normal),
        Arc::clone(&genesis),
        &settings,
    )
    .expect("normal genesis");
    crate::native_persist::persist_block_natives(
        Arc::clone(&fast),
        Arc::clone(&genesis),
        &settings,
    )
    .expect("fast genesis baseline");

    let mut prev = genesis;
    for height in 1..committee_count - 1 {
        prev = empty_child(&prev, height);
        crate::native_persist::persist_block_natives(
            Arc::clone(&normal),
            Arc::clone(&prev),
            &settings,
        )
        .expect("normal pre-run block");
        crate::native_persist::persist_block_natives(
            Arc::clone(&fast),
            Arc::clone(&prev),
            &settings,
        )
        .expect("fast pre-run block");
    }

    let mut blocks = Vec::new();
    for height in committee_count - 1..=committee_count * 2 {
        let block = empty_child(&prev, height);
        prev = Arc::clone(&block);
        blocks.push(block);
    }

    for block in &blocks {
        crate::native_persist::persist_block_natives(
            Arc::clone(&normal),
            Arc::clone(block),
            &settings,
        )
        .expect("normal empty block across refreshes");
    }

    let staged = stage_empty_block_fast_forward(
        Arc::clone(&fast),
        &blocks,
        &settings,
        bulk_options(),
        bulk_context(),
        &resources,
        committee_count - 2,
    )
    .expect("fast-forward across multiple committee refreshes");
    staged.commit();

    assert_eq!(
        visible_store_dump(&fast),
        visible_store_dump(&normal),
        "fast-forward must be byte-equivalent across multiple committee refreshes"
    );
}
