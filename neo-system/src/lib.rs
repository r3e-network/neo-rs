//! Node system orchestration: the canonical `NeoSystem` runtime, core actors
//! (blockchain, local node, task manager, transaction router), the P2P actor
//! layer, the event/handler system, the oracle service and the state-service
//! commit pipeline.
//!
//! # Module layout (task W3-C2)
//!
//! - [`neo_system`]: `NeoSystem` / `NeoSystemContext` orchestration runtime
//! - [`blockchain`]: the blockchain actor (block import/persist/relay)
//! - [`network::p2p`]: `LocalNode`, `RemoteNode`, `TaskManager`, `TaskSession`
//!   actors; the wire vocabulary they orchestrate is re-exported from
//!   `neo-core::network::p2p` so historical paths keep resolving
//! - [`events`]: `EventManager` plus the `i_event_handlers` traits
//! - [`oracle_service`]: oracle service shell over the `neo-oracle` engine
//! - [`state_service`]: state-root commit handlers wired into block persistence
//! - [`services`]: the authoritative `SystemContext` trait
//! - [`runtime`] / [`adapters`]: this crate's own lightweight actor runtime and
//!   dispatch adapters (unrelated to the `neo_actors` node runtime, which the
//!   moved actors bind to directly — task 215 / Wave D1-L1)

pub mod adapters;
pub mod blockchain;
pub mod events;
pub mod mempool;
pub mod network;
pub mod oracle_service;
pub mod runtime;
pub mod services;
pub mod state_service;

pub mod neo_system;

/// Event handler interfaces (consolidated in [`events::handlers`]).
///
/// Matches C# `Neo.IEventHandlers`. Kept at the historical module path from
/// `neo-core` so moved code and consumers keep resolving.
pub mod i_event_handlers {
    pub use crate::events::handlers::{
        CommittedHandler, CommittingHandler, MessageReceivedHandler, WalletChangedHandler,
    };
}

// Task 208: state root verification service moved from
// neo-core::state_service::verification; gated behind `state-verifier` so only
// consumers that register the verifier (e.g. neo-node) pay for it.
#[cfg(feature = "state-verifier")]
pub mod state_verification;

pub use events::{EventHandler, EventManager, PluginEvent, broadcast_plugin_event};
pub use services::SystemContext;

pub use neo_system::{
    NeoSystem, NeoSystemBuilder, NeoSystemContext, ReadinessStatus, STATE_STORE_SERVICE,
    ServiceRegistry, TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION,
    TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION, TransactionExecutionExceptionCategory,
    TransactionExecutionObservation, TransactionExecutionSink, TransactionExecutionStage,
    TransactionRouterHandle,
};

// Re-export MemoryPool from the merged mempool module
pub use mempool::MemoryPool;

#[cfg(feature = "state-verifier")]
pub use state_verification::StateServiceVerification;
