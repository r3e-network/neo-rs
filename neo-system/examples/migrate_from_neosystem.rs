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
use neo_system::Node;
use std::sync::Arc;

fn main() -> Result<(), String> {
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
    let storage: Arc<MemoryStore> = Arc::new(MemoryStore::new());
    let native_contract_provider = Arc::new(neo_native_contracts::StandardNativeProvider::new());
    let _node = Node::builder()
        .with_settings(settings.clone())
        .with_storage(storage)
        .with_blockchain(blockchain_handle)
        .with_network(network_handle)
        .with_native_contract_provider(native_contract_provider)
        .build()
        .map_err(|err| err.to_string())?;

    // 3. Application lifecycle remains outside `neo-system`. The daemon owns
    //    task supervision, cancellation, startup imports, and graceful
    //    shutdown; embedders should provide the same policy explicitly.

    Ok(())
}
