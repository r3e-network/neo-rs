//! Chain event handler for the runtime.

use crate::runtime::events::RuntimeEvent;
use neo_chain::ChainEvent;
use neo_core::state_service::StateStore;
use neo_state::StateTrieManager;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};

/// Processes chain events from the chain state controller.
pub async fn process_chain_events(
    rx: &mut broadcast::Receiver<ChainEvent>,
    event_tx: broadcast::Sender<RuntimeEvent>,
    state_trie: Arc<RwLock<StateTrieManager>>,
    state_store: Option<Arc<StateStore>>,
    shutdown_rx: &mut broadcast::Receiver<()>,
) {
    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        handle_chain_event(event, &event_tx, &state_trie, &state_store).await;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(target: "neo::runtime", lagged = n, "chain event receiver lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!(target: "neo::runtime", "chain event channel closed");
                        break;
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                info!(target: "neo::runtime", "chain event processor shutting down");
                break;
            }
        }
    }
}

async fn handle_chain_event(
    event: ChainEvent,
    event_tx: &broadcast::Sender<RuntimeEvent>,
    state_trie: &Arc<RwLock<StateTrieManager>>,
    state_store: &Option<Arc<StateStore>>,
) {
    match event {
        ChainEvent::BlockAdded {
            hash,
            height,
            on_main_chain,
        } => {
            if on_main_chain {
                info!(
                    target: "neo::runtime",
                    height,
                    hash = %hash,
                    "block added to main chain"
                );
                let hash_bytes: [u8; 32] = hash.to_bytes().try_into().unwrap_or([0u8; 32]);
                let _ = event_tx.send(RuntimeEvent::BlockApplied {
                    height,
                    hash: hash_bytes,
                });
            }
        }
        ChainEvent::TipChanged {
            new_hash,
            new_height,
            prev_hash,
        } => {
            info!(
                target: "neo::runtime",
                new_height,
                new_hash = %new_hash,
                prev_hash = %prev_hash,
                "chain tip changed"
            );
        }
        ChainEvent::Reorganization {
            fork_point,
            disconnected,
            connected,
        } => {
            handle_reorganization(
                fork_point,
                &disconnected,
                &connected,
                state_trie,
                state_store,
            )
            .await;
        }
        ChainEvent::GenesisInitialized { hash } => {
            info!(
                target: "neo::runtime",
                hash = %hash,
                "genesis block initialized"
            );
        }
    }
}

async fn handle_reorganization(
    fork_point: neo_core::UInt256,
    disconnected: &[neo_core::UInt256],
    connected: &[neo_core::UInt256],
    state_trie: &Arc<RwLock<StateTrieManager>>,
    state_store: &Option<Arc<StateStore>>,
) {
    warn!(
        target: "neo::runtime",
        fork_point = %fork_point,
        disconnected_count = disconnected.len(),
        connected_count = connected.len(),
        "chain reorganization detected, initiating state rollback"
    );

    if let Some(ref store) = state_store {
        let rollback_height = {
            let trie = state_trie.read().await;
            let current = trie.current_index();
            current.saturating_sub(disconnected.len() as u32)
        };

        let snapshot = store.get_snapshot();
        if let Some(fork_root) = snapshot.get_state_root(rollback_height) {
            info!(
                target: "neo::runtime",
                rollback_height,
                fork_root = %fork_root.root_hash,
                "rolling back state trie to fork point"
            );

            let mut trie = state_trie.write().await;
            trie.reset_to_root(fork_root.root_hash, rollback_height);

            info!(
                target: "neo::runtime",
                new_index = trie.current_index(),
                "state rollback complete, ready to apply new chain"
            );
        } else {
            warn!(
                target: "neo::runtime",
                rollback_height,
                "state root not found for rollback height, full resync may be needed"
            );
        }
    } else {
        debug!(
            target: "neo::runtime",
            "state store not enabled, skipping state rollback"
        );
    }

    info!(
        target: "neo::runtime",
        connected_count = connected.len(),
        "awaiting re-execution of {} connected blocks",
        connected.len()
    );
}
