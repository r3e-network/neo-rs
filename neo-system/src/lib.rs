//! # neo-system
//!
//! The Neo node orchestrator. This is the reth-style equivalent of
//! the C# `NeoSystem` class. The [`Node`] composes the various
//! service implementations and exposes them to consumers (RPC
//! server, consensus driver, plugins) as plain `async fn` calls on
//! the corresponding trait objects.
//!
//! ## Layering
//!
//! Sits in **Layer 5 (Composition)**. Depends on the node-service
//! crates (`neo-blockchain`, `neo-network`, `neo-wallets`, …), domain
//! service crates (`neo-mempool`, `neo-execution`, `neo-runtime`, …),
//! and lower protocol / infrastructure crates needed to wire them
//! together. The crate does **not** contain any
//! Akka-style actor code: every service interaction is a plain
//! `async fn` on a trait object, backed by a `mpsc::Sender<T>` and
//! optionally a `oneshot::Sender<Reply>` for request/response
//! flows.
//!
//! ## Usage
//!
//! Compose a [`Node`] from explicitly-constructed services:
//!
//! 1. Construct services explicitly:
//!
//!    ```no_run
//!    use neo_blockchain::BlockchainHandle;
//!    use neo_network::LocalNodeService;
//!
//!    # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//!    // Build a blockchain handle (the corresponding service is owned
//!    // by the caller — see the neo_blockchain docs for the full
//!    // `BlockchainService::new(...)` argument list).
//!    let (blockchain_handle, _blockchain_rx) = BlockchainHandle::with_capacity();
//!
//!    // Build a network service + handle pair.
//!    let (network_svc, network_handle) = LocalNodeService::new(Default::default());
//!    let _ = tokio::spawn(network_svc.run());
//!
//!    # let _ = (blockchain_handle, network_handle);
//!    # Ok(()) }
//!    ```
//!
//! 2. Hand the resulting handles to a [`Node`] via
//!    [`NodeBuilder::with_blockchain`] / [`NodeBuilder::with_network`].
//! 3. Call services through the [`Node`] (or directly through
//!    the handles) — never through an `ActorRef`.
//!
//! ## Re-export index
//!
//! | Item | Path | Purpose |
//! |------|------|---------|
//! | Node | [`Node`] | Composed runtime |
//! | Node builder | [`NodeBuilder`] | Fluent builder for [`Node`] |
//! | Wallet provider | [`WalletProvider`] | Thread-safe wallet handle |
//! | Node error | [`NodeError`] | Builder / lifecycle error vocabulary |

#![doc(html_root_url = "https://docs.rs/neo-system/0.8.0")]

pub mod builder;
pub mod error;
pub mod node;
pub mod service_registry;
pub mod wallet_provider;

// Public re-exports for the crate's public surface.
pub use builder::NodeBuilder;
pub use error::{NodeError, NodeResult};
pub use node::Node;
pub use service_registry::ServiceRegistry;
pub use wallet_provider::WalletProvider;
