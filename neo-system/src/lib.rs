//! # neo-system
//!
//! The Neo node orchestrator. This is the reth-style equivalent of
//! the legacy `NeoSystem` class. The [`Node`] composes the various
//! service implementations and exposes them to consumers (RPC
//! server, consensus driver, plugins) as plain `async fn` calls on
//! the corresponding trait objects.
//!
//! ## Layering
//!
//! Sits in **Layer 2 (service composition)**. Depends on the
//! service-layer crates (`neo-blockchain`, `neo-network`,
//! `neo-mempool`, `neo-execution`, …) and on the runtime service
//! traits (`neo-runtime`). The crate does **not** contain any
//! Akka-style actor code: every service interaction is a plain
//! `async fn` on a trait object, backed by a `mpsc::Sender<T>` and
//! optionally a `oneshot::Sender<Reply>` for request/response
//! flows.
//!
//! ## Migration path
//!
//! The legacy `neo_core::neo_system::NeoSystem` is being phased
//! out. New code should:
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
//! | Back-compat re-exports | [`legacy`] | Common type aliases for incremental migration |

#![doc(html_root_url = "https://docs.rs/neo-system/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod back_compat;
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

// Re-export common types from the foundation / service-layer crates
// so the migration from `neo_core::X` to the new home is purely
// mechanical: replace `neo_core::X` with `neo_system::X` and the
// code keeps compiling. The underlying types are the same
// (they were moved out of `neo-core` into the new canonical
// crates); this module is a re-export convenience only.
pub mod legacy {
    //! Back-compat re-exports of the common types that used to live
    //! at `neo_core::X` and have since moved to the canonical
    //! foundation / service-layer crates.
    //!
    //! Existing consumers can switch from `use neo_core::X;` to
    //! `use neo_system::legacy::X;` as a first step. The eventual
    //! goal is to import directly from the canonical crate
    //! (`use neo_primitives::X;`, `use neo_payloads::X;`, …).
    pub use neo_primitives::{BigDecimal, UInt160, UInt256};
    pub use neo_payloads::{Block, Header, Signer, Transaction};
    pub use neo_ledger_types::Witness;
    pub use neo_config::ProtocolSettings;
    pub use neo_error::{CoreError, CoreResult};
}
