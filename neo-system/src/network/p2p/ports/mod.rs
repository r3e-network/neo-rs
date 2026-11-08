//! Typed P2P ports (hexagonal boundary for the network/session layer).
//!
//! This module is the destination-only typed replacement for the
//! `NeoSystemContext` "Any joints" the networking stack used to reach ledger,
//! mempool, chain, task-manager, and dispatch collaborators. Every leaf names
//! only canonical protocol payloads, primitives, this crate's own types, `std`,
//! and `tokio` — never a core/system/native/ledger/storage or actor-runtime
//! type — so `neo-p2p` gains no reverse dependency on the layers it serves.
//!
//! # Leaves
//!
//! * [`peer_handle`] — opaque [`PeerHandle`] replacing `ActorRef`.
//! * [`ledger_query`] — [`LedgerQueryPort`], read-only canonical chain state.
//! * [`mempool`] — [`MempoolQueryPort`], in-memory relayable inventory.
//! * [`ingress`] — [`TransactionIngressPort`], transaction pre-verification.
//! * [`chain_sink`] — [`ChainSinkPort`], validated-inventory forwarding.
//! * [`task_manager`] — [`TaskManagerPort`], bounded sync-task commands.
//! * [`peer_command`] — [`PeerCommandPort`], outbound peer requests.
//! * [`persistence_events`] — [`PersistenceEventPort`] + neutral event values.
//! * [`dispatch`] — [`InboundDispatchPort`], external inbound-frame veto.
//!
//! # Bundles
//!
//! Adapters construct two `Arc<dyn ..>` bundles: [`RemoteSessionPorts`] for a
//! peer session and [`TaskManagerPorts`] for the task manager. The port traits
//! are intentionally synchronous and return [`PortResult`] so they stay
//! dyn-compatible for these bundles; the underlying actor sends they replace
//! were themselves synchronous mailbox `tell`s.
//!
//! # Wiring status
//!
//! Dest-only: this module is not yet declared from the crate root and the
//! `neo-protocol` dependency it names is not yet in the manifest. Task 89
//! registers `pub mod ports;` (and `pub mod sessions;`), adds the manifest
//! dependency, and hooks the adapters and callers. Nothing here compiles into
//! the crate until then.

pub mod chain_sink;
pub mod dispatch;
pub mod ingress;
pub mod ledger_query;
pub mod mempool;
pub mod peer_command;
pub mod peer_handle;
pub mod persistence_events;
pub mod task_manager;

pub use chain_sink::ChainSinkPort;
pub use dispatch::InboundDispatchPort;
pub use ingress::TransactionIngressPort;
pub use ledger_query::LedgerQueryPort;
pub use mempool::MempoolQueryPort;
pub use peer_command::PeerCommandPort;
pub use peer_handle::PeerHandle;
pub use persistence_events::{
    PersistFailure, PersistedBlock, PersistenceEvent, PersistenceEventPort, RelayOutcome,
};
pub use task_manager::TaskManagerPort;

use std::sync::Arc;

/// Failure mode for a port command that could not be honored.
///
/// Kept deliberately small and `std`-only (no `thiserror`): the ports may only
/// name the allowed dependency set, so the `Display`/`Error` impls are written
/// by hand. Every command port reports delivery problems through this type so
/// callers can apply their fail-closed rollback instead of assuming success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortError {
    /// A command/message could not be delivered to its collaborator
    /// (e.g. a closed or full mailbox). Carries a human-readable context
    /// string; the caller must treat the operation as not performed.
    Delivery(String),
    /// A [`PeerHandle`] no longer maps to a live registered session, so a
    /// peer-directed command has no target. The caller should drop the peer's
    /// local state rather than retry into a dead session.
    Unresolved(PeerHandle),
}

impl PortError {
    /// Builds a [`PortError::Delivery`] from any displayable context.
    pub fn delivery(message: impl Into<String>) -> Self {
        Self::Delivery(message.into())
    }

    /// Builds a [`PortError::Unresolved`] for a stale peer handle.
    pub fn unresolved(peer: PeerHandle) -> Self {
        Self::Unresolved(peer)
    }
}

impl std::fmt::Display for PortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Delivery(message) => write!(f, "port delivery failed: {message}"),
            Self::Unresolved(peer) => write!(f, "port target unresolved: {peer}"),
        }
    }
}

impl std::error::Error for PortError {}

/// Result alias used by every fallible port command.
pub type PortResult<T> = Result<T, PortError>;

/// Port bundle handed to a peer session (the `RemoteNode` side).
///
/// Covers everything a session needs to answer inbound frames and forward
/// validated inventory without naming a system, ledger, mempool, or actor
/// type. The task manager is reached through [`TaskManagerPort`] rather than an
/// `ActorRef`.
#[derive(Clone)]
pub struct RemoteSessionPorts {
    /// Read-only canonical chain-state queries.
    pub ledger_query: Arc<dyn LedgerQueryPort>,
    /// In-memory relayable-inventory queries (mempool + relay cache).
    pub mempool: Arc<dyn MempoolQueryPort>,
    /// Transaction pre-verification submission.
    pub ingress: Arc<dyn TransactionIngressPort>,
    /// Validated block/extensible/header forwarding to the chain actor.
    pub chain_sink: Arc<dyn ChainSinkPort>,
    /// Bounded sync-task commands to the task manager.
    pub task_manager: Arc<dyn TaskManagerPort>,
    /// External inbound-frame dispatch/veto hook.
    pub dispatch: Arc<dyn InboundDispatchPort>,
}

/// Port bundle handed to the task manager (the scheduling side).
///
/// Covers chain/mempool reads for sync-window planning, outbound peer requests,
/// and the persistence/relay event subscription that advances the frontier.
#[derive(Clone)]
pub struct TaskManagerPorts {
    /// Read-only canonical chain-state queries.
    pub ledger_query: Arc<dyn LedgerQueryPort>,
    /// In-memory relayable-inventory queries (mempool capacity for dedup sizing).
    pub mempool: Arc<dyn MempoolQueryPort>,
    /// Outbound requests/disconnects to scheduled peers.
    pub peer_command: Arc<dyn PeerCommandPort>,
    /// Persistence/relay event subscription.
    pub persistence_events: Arc<dyn PersistenceEventPort>,
}

// ---------------------------------------------------------------------------
// Compile-level contract assertions (no runtime cost, no `#[test]` needed).
//
// Each `Arc<dyn *Port>` field above already forces its trait to be
// object-safe (dyn-compatible); the block below additionally pins that every
// port trait object and both bundles are `Send + Sync`, so adapters can share
// them across tasks/threads exactly as the actor runtime did.
// ---------------------------------------------------------------------------
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}

    // Port trait objects are shareable across threads/tasks.
    assert_send_sync::<dyn LedgerQueryPort>();
    assert_send_sync::<dyn MempoolQueryPort>();
    assert_send_sync::<dyn TransactionIngressPort>();
    assert_send_sync::<dyn ChainSinkPort>();
    assert_send_sync::<dyn TaskManagerPort>();
    assert_send_sync::<dyn PeerCommandPort>();
    assert_send_sync::<dyn PersistenceEventPort>();
    assert_send_sync::<dyn InboundDispatchPort>();

    // The bundles and the opaque handle are shareable too.
    assert_send_sync::<PeerHandle>();
    assert_send_sync::<RemoteSessionPorts>();
    assert_send_sync::<TaskManagerPorts>();

    // Prove each trait is genuinely object-safe by naming a `dyn` coercion.
    fn _dyn_coercions(
        _a: Arc<dyn LedgerQueryPort>,
        _b: Arc<dyn MempoolQueryPort>,
        _c: Arc<dyn TransactionIngressPort>,
        _d: Arc<dyn ChainSinkPort>,
        _e: Arc<dyn TaskManagerPort>,
        _f: Arc<dyn PeerCommandPort>,
        _g: Arc<dyn PersistenceEventPort>,
        _h: Arc<dyn InboundDispatchPort>,
    ) {
    }
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_error_display_is_stable() {
        let delivery = PortError::delivery("mailbox closed");
        assert_eq!(delivery.to_string(), "port delivery failed: mailbox closed");
        assert_eq!(delivery, PortError::Delivery("mailbox closed".to_string()));

        let unresolved = PortError::unresolved(PeerHandle::new(9));
        assert_eq!(unresolved.to_string(), "port target unresolved: peer#9");
    }

    #[test]
    fn port_error_is_a_std_error() {
        // The hand-written impl must satisfy `std::error::Error` so adapters can
        // bubble a port failure through `?` into a boxed error.
        fn assert_error<E: std::error::Error>() {}
        assert_error::<PortError>();

        let err: Box<dyn std::error::Error> = Box::new(PortError::delivery("boom"));
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn port_result_alias_carries_port_error() {
        let ok: PortResult<()> = Ok(());
        assert!(ok.is_ok());

        let failed: PortResult<()> = Err(PortError::unresolved(PeerHandle::new(1)));
        assert!(matches!(
            failed,
            Err(PortError::Unresolved(peer)) if peer.as_u64() == 1
        ));
    }
}
