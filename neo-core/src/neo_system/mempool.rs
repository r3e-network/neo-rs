//! Mempool callback wiring for `NeoSystem`.
//!
//! Keeps the `core` constructor lean by encapsulating callback registration and
//! plugin/event notifications for mempool activity.

use parking_lot::Mutex;
use std::sync::Arc;

use crate::akka::ActorRef;
use tracing::debug;

use super::context::NeoSystemContext;
use crate::network::p2p::local_node::RelayInventory;
use crate::network::p2p::payloads::transaction::Transaction;
use crate::network::p2p::LocalNodeCommand;

/// Attaches callbacks to the mempool to surface events and relay transactions.
pub(crate) fn attach_mempool_callbacks(
    context: &Arc<NeoSystemContext>,
    memory_pool: &Arc<Mutex<crate::ledger::MemoryPool>>,
    local_node: ActorRef,
    blockchain: ActorRef,
) {
    let mut pool = memory_pool.lock();
    let context_added = context.clone();
    pool.transaction_added = Some(Box::new(move |sender, tx| {
        let handlers = { context_added.transaction_added_handlers().read().clone() };
        for handler in handlers {
            handler.memory_pool_transaction_added_handler(sender, tx);
        }
        context_added.broadcast_plugin_event(crate::events::PluginEvent::MempoolTransactionAdded {
            tx_hash: tx.hash().to_string(),
        });
    }));

    let context_removed = context.clone();
    pool.transaction_removed = Some(Box::new(move |sender, args| {
        let handlers = {
            context_removed
                .transaction_removed_handlers()
                .read()
                .clone()
        };
        for handler in handlers {
            handler.memory_pool_transaction_removed_handler(sender, args);
        }
        let hashes = args
            .transactions
            .iter()
            .map(|tx| tx.hash().to_string())
            .collect::<Vec<_>>();
        context_removed.broadcast_plugin_event(
            crate::events::PluginEvent::MempoolTransactionRemoved {
                tx_hashes: hashes,
                reason: format!("{:?}", args.reason),
            },
        );
    }));

    let local_node_ref = local_node.clone();
    let blockchain_ref = blockchain.clone();
    pool.transaction_relay = Some(Box::new(move |tx: &Transaction| {
        if let Err(error) = local_node_ref.tell_from(
            LocalNodeCommand::RelayDirectly {
                inventory: RelayInventory::Transaction(tx.clone()),
                block_index: None,
            },
            Some(blockchain_ref.clone()),
        ) {
            debug!(
                target: "neo",
                %error,
                "failed to enqueue relayed transaction from memory pool"
            );
        }
    }));
}
