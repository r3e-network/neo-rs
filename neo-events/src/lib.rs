//! # neo-events
//!
//! Typed event payloads and broadcast helpers for the Neo runtime.
//!
//! The original `Neo.Events` namespace in C# exposes a typed event
//! manager for unit-test wiring plus lightweight plugin event payloads
//! for internal broadcasting. The trait-style `*Handler` types
//! (`CommittedHandler`, `CommittingHandler`, `MessageReceivedHandler`,
//! `WalletChangedHandler`) are runtime-coupled to `Block` /
//! `Transaction` / `Wallet` / `DataCache` and stay in
//! `neo-core::events::handlers`; the pure event payloads
//! ([`PluginEvent`]) and the broadcast helper
//! ([`broadcast_plugin_event`]) now live here.
//!
//! ## Layering
//!
//! Sits in **Layer 1 (utility)**. Depends only on:
//! - `neo-primitives` (Layer 0) — placeholder for future hash types.
//! - `neo-ledger-types` (Layer 1) — placeholder for future event payloads.
//! - `tracing` — for the `broadcast_plugin_event` debug log.
//!
//! Must **not** depend on `neo-core` (Layer 2 runtime), `neo-storage`
//! (state caches), or any native contract / smart-contract engine. This
//! matches the polkadot-sdk / reth rule that event payloads live in a
//! tiny leaf crate that every layer can depend on without inverting the
//! dependency graph.

#![doc(html_root_url = "https://docs.rs/neo-events/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

use std::any::Any;
use std::sync::Arc;
use tracing::debug;

/// Lightweight plugin event enum for internal event broadcasting.
/// Replaces the previous plugin system with simple logging.
pub enum PluginEvent {
    /// Node has started with system reference
    NodeStarted {
        /// Reference to the NeoSystem (or other system implementation)
        system: Arc<dyn Any + Send + Sync>,
    },
    /// Node is stopping
    NodeStopping,
    /// A block was received
    BlockReceived {
        /// Block hash as a hex string.
        block_hash: String,
        /// Block height.
        block_height: u32,
    },
    /// A transaction was received
    TransactionReceived {
        /// Transaction hash as a hex string.
        tx_hash: String,
    },
    /// Transaction added to mempool
    MempoolTransactionAdded {
        /// Transaction hash as a hex string.
        tx_hash: String,
    },
    /// Transactions removed from mempool
    MempoolTransactionRemoved {
        /// Transaction hashes as hex strings.
        tx_hashes: Vec<String>,
        /// Removal reason (matches C# `TransactionRemovalReason` stringified).
        reason: String,
    },
    /// A service was added
    ServiceAdded {
        /// Service name.
        service_name: String,
    },
    /// Wallet changed
    WalletChanged {
        /// Wallet name.
        wallet_name: String,
    },
}

impl std::fmt::Debug for PluginEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginEvent::NodeStarted { .. } => write!(f, "NodeStarted {{ system: ... }}"),
            PluginEvent::NodeStopping => write!(f, "NodeStopping"),
            PluginEvent::BlockReceived {
                block_hash,
                block_height,
            } => f
                .debug_struct("BlockReceived")
                .field("block_hash", block_hash)
                .field("block_height", block_height)
                .finish(),
            PluginEvent::TransactionReceived { tx_hash } => f
                .debug_struct("TransactionReceived")
                .field("tx_hash", tx_hash)
                .finish(),
            PluginEvent::MempoolTransactionAdded { tx_hash } => f
                .debug_struct("MempoolTransactionAdded")
                .field("tx_hash", tx_hash)
                .finish(),
            PluginEvent::MempoolTransactionRemoved { tx_hashes, reason } => f
                .debug_struct("MempoolTransactionRemoved")
                .field("tx_hashes", tx_hashes)
                .field("reason", reason)
                .finish(),
            PluginEvent::ServiceAdded { service_name } => f
                .debug_struct("ServiceAdded")
                .field("service_name", service_name)
                .finish(),
            PluginEvent::WalletChanged { wallet_name } => f
                .debug_struct("WalletChanged")
                .field("wallet_name", wallet_name)
                .finish(),
        }
    }
}

impl std::fmt::Display for PluginEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginEvent::NodeStarted { .. } => write!(f, "NodeStarted"),
            PluginEvent::NodeStopping => write!(f, "NodeStopping"),
            PluginEvent::BlockReceived {
                block_hash,
                block_height,
            } => {
                write!(f, "BlockReceived({}, height={})", block_hash, block_height)
            }
            PluginEvent::TransactionReceived { tx_hash } => {
                write!(f, "TransactionReceived({})", tx_hash)
            }
            PluginEvent::MempoolTransactionAdded { tx_hash } => {
                write!(f, "MempoolTransactionAdded({})", tx_hash)
            }
            PluginEvent::MempoolTransactionRemoved { tx_hashes, reason } => {
                write!(f, "MempoolTransactionRemoved({:?}, {})", tx_hashes, reason)
            }
            PluginEvent::ServiceAdded { service_name } => {
                write!(f, "ServiceAdded({})", service_name)
            }
            PluginEvent::WalletChanged { wallet_name } => {
                write!(f, "WalletChanged({})", wallet_name)
            }
        }
    }
}

/// Broadcasts a plugin event by logging it.
/// This is a no-op replacement for the removed plugin system.
#[inline]
pub fn broadcast_plugin_event(event: &PluginEvent) {
    debug!(target: "neo::events", event = %event, "plugin event");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_does_not_leak_system_reference() {
        let event = PluginEvent::NodeStarted {
            system: Arc::new(()) as Arc<dyn Any + Send + Sync>,
        };
        let formatted = format!("{:?}", event);
        assert!(formatted.contains("NodeStarted"));
        // The opaque system reference is intentionally elided.
        assert!(!formatted.contains("()"));
    }

    #[test]
    fn display_round_trip() {
        let event = PluginEvent::BlockReceived {
            block_hash: "0xabcd".to_string(),
            block_height: 42,
        };
        let formatted = format!("{}", event);
        assert!(formatted.contains("BlockReceived"));
        assert!(formatted.contains("0xabcd"));
        assert!(formatted.contains("42"));
    }
}
