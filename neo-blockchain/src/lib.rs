//! # neo-blockchain
//!
//! Reth-style blockchain service for the Neo node.
//!
//! The blockchain is the only subsystem in the node that is *command-shaped*
//! rather than *method-shaped*: a single async command loop owns every
//! state transition, the rest of the node talks to it through a typed
//! [`BlockchainHandle`] that wraps a `tokio::sync::mpsc::Sender<BlockchainCommand>`
//! and a `tokio::sync::broadcast::Sender<BlockchainEvent>`.
//!
//! The service pattern replaces the previous Akka-style actor implementation
//! that used to live in `neo-core/src/ledger/blockchain`. The actor trait
//! implementation, the `Actor` / `ActorContext` plumbing, and the per-message
//! `tell()` / `ask()` boilerplate have all been replaced with:
//!
//! 1. A plain `struct BlockchainService` that owns the command channel and
//!    the broadcast channel.
//! 2. An `async fn BlockchainService::run(self)` that loops over
//!    `cmd_rx.recv().await` and dispatches to `async fn` handler methods on
//!    the struct (no trait objects, no `Box<dyn Any>` downcasting).
//! 3. A cheap-to-clone [`BlockchainHandle`] that other subsystems store in
//!    their state. The handle is the only public surface of the service —
//!    the [`BlockchainService`] is constructed once and immediately
//!    `tokio::spawn`'d by the node binary.
//!
//! ## Module layout
//!
//! | Module | Purpose |
//! |--------|---------|
//! | [`blockchain`] | Re-exports of the runtime's [`BlockchainCommand`] / [`BlockchainEvent`] / [`BlockchainHandle`] |
//! | [`command`]    | The actor's *internal* command enum (the comprehensive set the actor's old mailbox used) |
//! | [`handle`]     | The service handle — `BlockchainHandle::new()` returns a fresh `(handle, cmd_rx, event_tx)` triple |
//! | [`internal`]   | Internal types: `UnverifiedBlocksList`, `ImportDisposition`, classify helpers |
//! | [`block_processing`] | Block verification + persistence loop |
//! | [`handlers`]   | The `impl BlockchainService` block that wires command variants to async fn handlers |
//! | [`transaction`]| Transaction admission / mempool interactions |
//! | [`import`], [`import_completed`], [`fill_memory_pool`], [`fill_completed`], [`persist_completed`], [`relay_result`], [`reverify`], [`inventory_payload`] | Per-message types used by the command enum |
//! | [`ledger_context`], [`header_cache`] | The in-memory ledger caches the service uses for hot lookups |
//!
//! ## Service contract
//!
//! ```text
//! ┌──────────────────┐  mpsc::Sender<BlockchainCommand>   ┌────────────────────┐
//! │  Other node      │ ────────────────────────────────▶ │  BlockchainService │
//! │  subsystems      │ ◀── broadcast::Receiver<Event> ── │  (tokio::spawn)    │
//! └──────────────────┘                                   └────────────────────┘
//! ```
//!
//! The `BlockchainService` is a *single* owner of the command channel. All
//! state mutations (block import, mempool fill, reverify, …) are processed
//! sequentially by the `run()` loop, which is the property the old actor
//! gave us, but expressed in plain `async` / `await` rather than an actor
//! framework.

#![doc(html_root_url = "https://docs.rs/neo-blockchain/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod block_processing;
pub mod command;
pub mod fill_completed;
pub mod fill_memory_pool;
pub mod handle;
pub mod handlers;
pub mod header_cache;
pub mod import;
pub mod import_completed;
pub mod internal;
pub mod inventory_payload;
pub mod ledger_context;
pub mod persist_completed;
pub mod relay_result;
pub mod reverify;
pub mod service;
pub mod service_context;
pub mod transaction;

// Re-exports for the public surface of the crate.
//
// The runtime crate (`neo-runtime`) already owns the *trait-level* service
// types — `BlockchainCommand` (request/response), `BlockchainEvent`, and
// `BlockchainHandle` (the mpsc / broadcast channel wrapper). The canonical
// home for those types is `neo-runtime`; we re-export them here so the
// crate's public surface is self-contained.
pub mod blockchain {
    //! Re-exports of the runtime's blockchain service types.
    pub use neo_runtime::{
        BlockchainCommand as RuntimeBlockchainCommand, BlockchainEvent as RuntimeBlockchainEvent,
        BlockchainHandle as RuntimeBlockchainHandle, DEFAULT_COMMAND_CAPACITY,
        DEFAULT_EVENT_CAPACITY,
    };
}

pub use command::BlockchainCommand;
pub use neo_runtime::BlockchainEvent;
pub use fill_completed::FillCompleted;
pub use fill_memory_pool::FillMemoryPool;
pub use handle::BlockchainHandle;
pub use import::Import;
pub use import_completed::ImportCompleted;
pub use internal::{ImportDisposition, UnverifiedBlocksList};
pub use inventory_payload::InventoryPayload;
pub use persist_completed::PersistCompleted;
// `PreverifyCompleted` is produced by `neo-mempool`'s transaction router and
// only consumed here; re-export the single canonical definition rather than
// duplicating the record. (neo-blockchain depends on neo-mempool.)
pub use neo_mempool::PreverifyCompleted;
pub use relay_result::RelayResult;
pub use reverify::{Reverify, ReverifyItem};
pub use command::AddTransactionReply;
pub use service::{Blockchain, BlockchainService};

pub use neo_runtime::{
    BlockchainCommand as RuntimeCommand, BlockchainEvent as RuntimeEvent,
    BlockchainHandle as RuntimeHandle, ServiceError,
};

pub use header_cache::HeaderCache;
pub use ledger_context::LedgerContext;
