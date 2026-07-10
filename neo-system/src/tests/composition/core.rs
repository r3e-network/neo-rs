use std::sync::Arc;

use neo_blockchain::LedgerContext;
use neo_config::ProtocolSettings;
use neo_native_contracts::StandardNativeProvider;
use neo_network::NetworkHandle;
use neo_storage::persistence::providers::MemoryStore;

use super::*;
use crate::NoopBlockCommitHooks;

#[test]
fn core_builder_preserves_one_typed_component_graph() {
    let settings = Arc::new(ProtocolSettings::default());
    let storage = Arc::new(MemoryStore::new());
    let provider = Arc::new(StandardNativeProvider::new());
    let launch = NodeCoreBuilder::new(
        Arc::clone(&settings),
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
    let node = core
        .into_node(network)
        .expect("the staged core contains every required node component");

    assert!(Arc::ptr_eq(&settings, &node.settings()));
    assert!(Arc::ptr_eq(&storage, &node.storage()));
    assert!(Arc::ptr_eq(&provider, &node.native_contract_provider()));
    assert!(Arc::ptr_eq(
        &provider,
        &node.mempool().native_contract_provider()
    ));
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
    assert!(
        !builder.contains("CancellationToken::new()"),
        "NodeBuilder must not manufacture an unused lifecycle token"
    );
}

#[tokio::test]
async fn blockchain_task_runs_behind_the_named_launch_boundary() {
    let launch = NodeCoreBuilder::new(
        Arc::new(ProtocolSettings::default()),
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
