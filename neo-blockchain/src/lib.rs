//! # neo-blockchain
//!
//! Concrete block import, validation, persistence, and hot ledger context for
//! Neo N3.
//!
//! ## Boundary
//!
//! This node-service crate owns the concrete block-import path and must not
//! depend upward on composition, RPC, GUI, or binaries.
//!
//! ## Contents
//!
//! - `ledger`: Ledger caches, lookup context, and persisted record helpers used
//!   by block import.
//! - `messages`: Typed service commands, events, and payload wrappers for the
//!   crate boundary.
//! - `pipeline`: Ordered validation, execution, native-hook, and persistence
//!   steps for block import.
//! - `service`: Service loops, handles, lifecycle helpers, and command
//!   processing.

#![doc(html_root_url = "https://docs.rs/neo-blockchain/0.10.0")]

/// Ledger caches and persisted ledger-record helpers used by the service loop.
pub mod ledger;
pub mod messages;
/// Block import, validation, handler, and native-persistence pipeline.
pub mod pipeline;
/// Command-loop service, handle, and context traits.
pub mod service;
/// Active signed-StateRoot consensus: StateValidator vote signing + aggregation.
pub mod state_root_consensus;
/// Signed StateRoot witness verification against the StateValidators multisig.
pub mod state_root_verify;

pub use state_root_consensus::{
    StateRootVoteCollector, aggregate_state_root_witness, sign_state_root, validate_state_root_vote,
};
pub use state_root_verify::verify_state_root_with_native_provider;

pub(crate) use ledger::ledger_records;
pub use ledger::{header_cache, ledger_context, ledger_provider};
pub use pipeline::{
    block_processing, block_validation, empty_block_fast_forward, handlers, native_persist,
    validate_stage,
};
pub use service::{command, handle, internal, service_context};

pub use messages::{
    fill_completed, fill_memory_pool, import, import_completed, inventory_payload,
    persist_completed, relay_result, reverify,
};

// Re-exports for the public surface of the crate.
//
// The runtime crate (`neo-runtime`) owns trait-level service contracts and
// broadcast event defaults. `neo-blockchain` owns the concrete command loop,
// command enum, and handle because it is the only crate allowed to translate
// public typed methods into service-loop commands.
pub mod blockchain {
    //! Re-exports of the runtime's shared blockchain types. The command channel
    //! and handle are owned by this crate (`BlockchainCommand` / `handle.rs`);
    //! `neo-runtime` contributes only the broadcast event and the default
    //! channel capacities shared by both.
    pub use neo_runtime::{
        BlockchainEvent as RuntimeBlockchainEvent, DEFAULT_COMMAND_CAPACITY, DEFAULT_EVENT_CAPACITY,
    };
}

pub use command::BlockchainCommand;
pub use fill_completed::FillCompleted;
pub use fill_memory_pool::FillMemoryPool;
pub use handle::BlockchainHandle;
pub use import::Import;
pub use import_completed::ImportCompleted;
pub use internal::{ImportDisposition, UnverifiedBlocksList};
pub use inventory_payload::InventoryPayload;
pub use native_persist::{
    NativePersistNotification, NativePersistOptions, NativePersistOutcome, NativePersistResources,
    chain_state_initialized, genesis_block, persist_block_natives_with_resources,
    stage_block_natives_with_resources,
};
pub use neo_runtime::BlockchainEvent;
pub use persist_completed::PersistCompleted;
// `PreverifyCompleted` is produced by `neo-mempool`'s transaction router and
// only consumed here; re-export the single canonical definition rather than
// duplicating the record. (neo-blockchain depends on neo-mempool.)
pub use command::AddTransactionReply;
pub use neo_mempool::PreverifyCompleted;
pub use relay_result::RelayResult;
pub use reverify::{Reverify, ReverifyItem};
pub use service::service::BlockchainService;

pub use neo_runtime::{BlockchainEvent as RuntimeEvent, ServiceError};

pub use header_cache::HeaderCache;
pub use ledger_context::LedgerContext;
pub use ledger_provider::{BlockProvider, StorageLedgerProvider, TxProvider};
