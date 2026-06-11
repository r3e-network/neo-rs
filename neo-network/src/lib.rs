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
//! workspace should depend on. The legacy Akka-style actor types
//! that lived in `neo_core::network::p2p::{local_node, remote_node,
//! task_manager}` are **re-exported** below for back-compat with the
//! existing consumers (neo-rpc, neo-node, neo-consensus) and will
//! be removed in Stage F once consumer migration is complete.
//!
//! ## Layering
//!
//! Sits in **Layer 1 (service)**. Depends on:
//!
//! - `neo-runtime` (Layer 0 / 1) — `NetworkService` trait, `Service`
//!   marker, `ServiceError`, `NetworkEvent`.
//! - `neo-payloads` / `neo-ledger-types` / `neo-primitives` (Layer 1)
//!   — `Block`, `Transaction`, `UInt256`.
//! - `neo-config` — `ProtocolSettings`.
//! - `neo-blockchain` / `neo-mempool` (Layer 2) — the services the
//!   network host talks to when receiving blocks / transactions.
//! - `neo-core` (legacy) — re-exports of the actor types; removed
//!   in Stage F.
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
//! ## Back-compat (Stage F will remove)
//!
//! The legacy Akka-style actor types are re-exported so the existing
//! consumers keep compiling unchanged.
//!
//! | Legacy type | New type |
//! |-------------|----------|
//! | `neo_core::network::p2p::local_node::LocalNodeHandle` | [`handle::NetworkHandle`] |
//! | `neo_core::network::p2p::remote_node::RemoteNodeHandle` | [`remote_node::RemoteNodeHandle`] |
//! | `neo_core::network::p2p::task_manager::TaskManagerHandle` | [`task_manager::TaskManagerHandle`] |
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

#![doc(html_root_url = "https://docs.rs/neo-network/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod command;
pub mod connection_timeouts;
pub mod error;
pub mod event;
pub mod handle;
pub mod local_identity;
pub mod local_node;
pub mod peer_id;
pub mod peer_registry;
pub mod remote_node;
pub mod task_manager;

// -----------------------------------------------------------------------------
// Public re-exports
// -----------------------------------------------------------------------------

pub use command::NetworkCommand;
pub use connection_timeouts::ConnectionTimeouts;
pub use error::{NetworkError, NetworkResult};
pub use event::NetworkEvent;
pub use handle::{
    NetworkHandle, SharedNetworkHandle, DEFAULT_COMMAND_CAPACITY, DEFAULT_EVENT_CAPACITY,
};
pub use local_identity::LocalIdentity;
pub use local_node::LocalNodeService;
pub use neo_p2p::ChannelsConfig;
pub use peer_id::PeerId;
pub use peer_registry::PeerRegistry;
pub use remote_node::{
    BlockSource, InboundInventory, InventoryItem, RemoteNodeCommand, RemoteNodeHandle,
    RemoteNodeService, RemoteNodeState,
};
pub use task_manager::{
    SyncTask, SyncTaskKind, TaskId, TaskManagerCommand, TaskManagerHandle, TaskManagerService,
};
