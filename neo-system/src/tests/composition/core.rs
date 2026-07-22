use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use neo_blockchain::{LedgerContext, OptionalStaticLedgerProvider, StaticLedgerArchiveFactory};
use neo_native_contracts::StandardNativeProvider;
use neo_network::NetworkHandle;
use neo_storage::persistence::providers::MemoryStore;

use super::*;
use crate::NoopBlockCommitHooks;

static ARCHIVE_ID: AtomicU64 = AtomicU64::new(0);

#[test]
fn core_builder_preserves_one_typed_component_graph() {
    let chain_spec = neo_config::NeoChainSpec::mainnet().expect("mainnet chain spec");
    let storage = Arc::new(MemoryStore::new());
    let provider = Arc::new(StandardNativeProvider::new());
    let launch = NodeCoreBuilder::new(
        Arc::clone(&chain_spec),
        neo_mempool::TxPoolConfig::default(),
        Arc::clone(&storage),
        Arc::clone(&provider),
        Arc::new(NoopBlockCommitHooks),
        42,
    )
    .build();
    let (core, _blockchain_task) = launch.into_parts();

    assert_eq!(core.persisted_height(), 42);
    assert_eq!(LedgerContext::current_height(&core.ledger_context()), 42);
    assert!(Arc::ptr_eq(
        &provider,
        &core.mempool().native_contract_provider()
    ));

    let (network, _commands, _events) = NetworkHandle::channel(8, 8);
    let node = core.into_node(network);

    assert!(Arc::ptr_eq(&chain_spec, &node.chain_spec()));
    assert!(Arc::ptr_eq(&storage, &node.storage()));
    assert!(Arc::ptr_eq(&provider, &node.native_contract_provider()));
    assert!(Arc::ptr_eq(
        &provider,
        &node.mempool().native_contract_provider()
    ));
}

#[test]
fn core_builder_preserves_the_configured_cold_ledger_provider() {
    let id = ARCHIVE_ID.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "neo-system-ledger-archive-{}-{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create archive directory");
    let archive = StaticLedgerArchiveFactory::default()
        .open(directory.join("ledger.static"))
        .expect("open static Ledger archive");
    let cold = OptionalStaticLedgerProvider::from_option(Some(archive.provider()));
    drop(archive);

    let launch = NodeCoreBuilder::new(
        neo_config::NeoChainSpec::mainnet().expect("mainnet chain spec"),
        neo_mempool::TxPoolConfig::default(),
        Arc::new(MemoryStore::new()),
        Arc::new(StandardNativeProvider::new()),
        Arc::new(NoopBlockCommitHooks),
        0,
    )
    .with_cold_ledger_provider(cold)
    .build();
    let (core, blockchain_task) = launch.into_parts();
    let (network, _commands, _events) = NetworkHandle::channel(8, 8);
    let node = core.into_node(network);

    assert!(node.ledger_provider_factory().cold().is_enabled());

    drop(node);
    drop(blockchain_task);
    std::fs::remove_dir_all(directory).expect("remove archive directory");
}

#[test]
fn composed_node_does_not_own_the_application_shutdown_token() {
    let node = include_str!("../../composition/node.rs");
    let builder = include_str!("../../composition/builder.rs");

    assert!(
        !node.contains("CancellationToken"),
        "process lifecycle cancellation belongs to the application supervisor"
    );
    assert!(
        !node.contains("cancellation_token("),
        "Node must not expose a second shutdown authority"
    );
    assert!(!builder.contains("CancellationToken::new()"));
}

#[tokio::test]
async fn blockchain_task_runs_behind_the_named_launch_boundary() {
    let launch = NodeCoreBuilder::new(
        neo_config::NeoChainSpec::mainnet().expect("mainnet chain spec"),
        neo_mempool::TxPoolConfig::default(),
        Arc::new(MemoryStore::new()),
        Arc::new(StandardNativeProvider::new()),
        Arc::new(NoopBlockCommitHooks),
        0,
    )
    .build();
    let (core, blockchain_task) = launch.into_parts();
    let blockchain = core.blockchain();
    let task = tokio::spawn(blockchain_task.run());

    blockchain
        .shutdown()
        .await
        .expect("the named task should own the blockchain command loop");
    task.await.expect("blockchain task should stop cleanly");
}
