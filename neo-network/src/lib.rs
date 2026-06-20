//! # neo-network
//!
//! Reth-style P2P network host for the Neo node.
//!
//! This crate is the **canonical home** for the three services that
//! make up the network host:
//!
//! - [`local_node::LocalNodeService`] — TCP accept loop, peer
//!   registry, and the entry point for outbound connections.
//! - [`remote_node::RemoteNodeService`] — per-peer state machine,
//!   one task per accepted TCP connection.
//! - [`task_manager::TaskManagerService`] — sync task orchestrator
//!   that tracks in-flight inventory requests.
//!
//! The trait-level contract is in
//! [`neo_runtime::NetworkService`]; the concrete implementations in
//! this crate are the only `impl NetworkService` the rest of the
//! workspace should depend on. Low-level P2P protocol primitives live
//! in [`proto`] and the wire envelope lives in [`wire`], so callers do
//! not need a separate P2P crate.
//!
//! ## Layering
//!
//! Sits in **Layer 4 (Node services)**. Depends on:
//!
//! - `neo-runtime` (Layer 3) — `NetworkService` trait, `Service`
//!   marker, `ServiceError`, `NetworkEvent`.
//! - `neo-payloads` (Layer 2) / `neo-primitives` (Layer 0) —
//!   `Block`, `Transaction`, `UInt256`.
//! - `neo-config` (Layer 1) — `ProtocolSettings`.
//! - `neo-blockchain` (Layer 4) / `neo-mempool` (Layer 3) — services the
//!   network host talks to when receiving blocks / transactions.
//! - `tokio`, `async-trait`, `futures`, `parking_lot`, `tracing`,
//!   `thiserror` — external async / utility crates.
//!
//! ## Service pattern
//!
//! Each service follows the reth pattern:
//!
//! 1. Construction goes through a `::new()` that returns a
//!    `(service, handle)` pair.
//! 2. The service is moved into a `tokio::spawn`'d task that
//!    drives `pub async fn run(self)` — a single
//!    `while let Some(cmd) = self.cmd_rx.recv().await` loop
//!    dispatching typed [`NetworkCommand`] variants to private
//!    `async fn` handlers.
//! 3. The handle is `Clone`, `Send`, and `Sync`, and is what the
//!    rest of the node stores. Public API on the handle is a
//!    request/response shape (e.g.
//!    [`handle::NetworkHandle::broadcast_block`]) backed by
//!    `mpsc::Sender<NetworkCommand>` + `oneshot::Sender<Reply>`.
//! 4. Events are published on a
//!    `tokio::sync::broadcast::Sender<NetworkEvent>` that
//!    consumers subscribe to via
//!    [`handle::NetworkHandle::subscribe`].
//!
//! ## Re-export index
//!
//! | Item | Path | Purpose |
//! |------|------|---------|
//! | Local node service | [`local_node::LocalNodeService`] | TCP accept loop |
//! | Remote node service | [`remote_node::RemoteNodeService`] | Per-peer state machine |
//! | Task manager service | [`task_manager::TaskManagerService`] | Sync task orchestrator |
//! | Network command | [`command::NetworkCommand`] | Top-level command enum |
//! | Network event | [`event::NetworkEvent`] | Event broadcast payload |
//! | Network handle | [`handle::NetworkHandle`] | Cheap-to-clone service handle |
//! | Network error | [`error::NetworkError`] | Service-specific error type |
//!
//! ## Quick start
//!
//! ```no_run
//! use std::sync::Arc;
//! use neo_config::ProtocolSettings;
//! use neo_network::{LocalNodeService, NetworkHandle};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let settings = Arc::new(ProtocolSettings::default());
//! let (service, handle) = LocalNodeService::new(settings);
//! let _task = tokio::spawn(service.run());
//! // handle.start("127.0.0.1:10333".parse()?).await?;
//! # Ok(()) }
//! ```

#![doc(html_root_url = "https://docs.rs/neo-network/0.8.0")]

pub mod command;
pub mod connection_timeouts;
pub mod error;
pub mod event;
pub mod handle;
pub mod local_identity;
pub mod local_node;
pub mod peer_id;
pub mod peer_registry;
pub mod proto;
pub mod remote_node;
pub mod task_manager;
pub mod wire;

// -----------------------------------------------------------------------------
// Public re-exports
// -----------------------------------------------------------------------------

pub use command::NetworkCommand;
pub use connection_timeouts::ConnectionTimeouts;
pub use error::{NetworkError, NetworkResult};
pub use event::NetworkEvent;
pub use handle::{
    DEFAULT_COMMAND_CAPACITY, DEFAULT_EVENT_CAPACITY, NetworkHandle, SharedNetworkHandle,
};
pub use local_identity::LocalIdentity;
pub use local_node::LocalNodeService;
pub use peer_id::PeerId;
pub use peer_registry::PeerRegistry;
pub use remote_node::{
    BlockSource, InboundInventory, InventoryItem, RemoteNodeCommand, RemoteNodeHandle,
    RemoteNodeService, RemoteNodeState,
};
pub use task_manager::{
    SyncTask, SyncTaskKind, TaskId, TaskManagerCommand, TaskManagerHandle, TaskManagerService,
};
pub use wire::{
    Message, MessageCodec, MessageHeader, NetworkMessage, PAYLOAD_MAX_SIZE, ProtocolMessage,
    WireError, WireResult,
};

// -----------------------------------------------------------------------------
// P2P protocol primitives re-export
//
// The former standalone P2P shim was folded into `neo-network` (it was almost
// entirely a re-export layer over `neo-primitives`). Its types now live under
// the `proto` submodule and are re-exported here at the crate root.
// -----------------------------------------------------------------------------

pub use proto::{
    ChannelsConfig, ContainsTransactionType, InvalidWitnessScopeError, InventoryType,
    MessageCommand, MessageFlags, NodeCapabilityType, OracleResponseCode, P2PError, P2PResult,
    TransactionAttributeType, TransactionRemovalReason, VerifyResult, WitnessConditionType,
    WitnessRuleAction, WitnessScope,
};
