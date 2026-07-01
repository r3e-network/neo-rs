use super::*;

use std::sync::{Arc, Mutex, MutexGuard};

use neo_config::{Hardfork, ProtocolSettings};
use neo_payloads::{Block, Header, Transaction};

use crate::empty_block_fast_forward::EmptyBlockFastForwardRequest;
use crate::native_persist::{NativePersistOptions, NativePersistResources};
use crate::service_context::BlockPersistContext;

static PROVIDER_TEST_LOCK: Mutex<()> = Mutex::new(());

fn lock_provider() -> MutexGuard<'static, ()> {
    PROVIDER_TEST_LOCK.lock().expect("provider test lock")
}

fn install_resources() -> NativePersistResources {
    let _ = neo_native_contracts::install();
    NativePersistResources::from_installed_provider().expect("native resources")
}

fn empty_block(index: u32) -> Arc<Block> {
    let mut header = Header::new();
    header.set_index(index);
    Arc::new(Block::from_parts(header, Vec::new()))
}

fn non_empty_block(index: u32) -> Arc<Block> {
    let mut header = Header::new();
    header.set_index(index);
    Arc::new(Block::from_parts(header, vec![Transaction::new()]))
}

fn bulk_options() -> NativePersistOptions {
    NativePersistOptions {
        capture_replay_artifacts: false,
    }
}

fn bulk_context() -> BlockPersistContext {
    BlockPersistContext::bulk_sync()
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
            EmptyBlockFastForwardRejection::NativeInitializationHeight {
                height: 2,
                ..
            }
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
        .map(|contract| (contract.name().to_string(), contract.supports_empty_block_fast_forward()))
        .collect::<Vec<_>>();

    assert_eq!(names.len(), neo_native_contracts::STANDARD_NATIVE_CONTRACT_COUNT);
    assert!(
        names.iter().all(|(_, supported)| *supported),
        "all active standard natives must explicitly opt in: {names:?}"
    );
}
