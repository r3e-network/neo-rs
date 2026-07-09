//! Migration example: `NeoSystem` → `Node`.
//!
//! This example shows how to translate the legacy
//! `NeoSystem::new(protocol_settings, …)` pattern into the new
//! `Node::builder()` pattern.
//!
//! The example does not actually run a real blockchain service
//! (those require a backing ledger / header cache / mempool). It
//! just constructs the handles via their low-level `with_capacity()` /
//! `channel()` constructors and the [`Node`] to show the migration path.

use neo_blockchain::BlockchainHandle;
use neo_config::ProtocolSettings;
use neo_network::NetworkHandle;
use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_storage::persistence::store::Store;
use neo_system::Node;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = Arc::new(ProtocolSettings::default());

    // ---- Legacy pattern (the code we are migrating from) ----
    //
    // ```ignore
    // use neo_core::neo_system::NeoSystem;
    // let system = NeoSystem::new(protocol_settings, None, None).await?;
    // system.blockchain_actor.tell(cmd).await?; // legacy untyped command send
    // system.shutdown().await?;
    // ```

    // ---- New pattern ----
    // 1. Construct each service explicitly. In production you'd
    //    call e.g. `BlockchainService::new(...)` to get a real
    //    service backed by a ledger; in this example we use the
    //    low-level channel constructors to avoid the heavy
    //    dependencies.
    let (blockchain_handle, _bc_rx) = BlockchainHandle::with_capacity();
    let (network_handle, _net_rx, _net_event_tx) = NetworkHandle::channel(1024, 1024);

    // 2. Compose the Node from the handles. The required
    //    parameters are: settings, storage, blockchain handle,
    //    network handle.
    let storage: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let native_contract_provider = Arc::new(neo_native_contracts::StandardNativeProvider::new());
    let node = Node::builder()
        .with_settings(settings.clone())
        .with_storage(storage)
        .with_blockchain(blockchain_handle)
        .with_network(network_handle)
        .with_native_contract_provider(native_contract_provider)
        .build()?;

    // 3. Drive the node lifecycle. `Node::run` blocks until the
    //    cancellation token is fired.
    let shutdown = node.cancellation_token();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        shutdown.cancel();
    });
    node.run().await?;

    Ok(())
}
