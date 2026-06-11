//! # neo-runtime
//!
//! Reth-style async service architecture for the Neo node.
//!
//! This crate is the **service pattern** for the rest of the workspace.
//! Every long-running component of a Neo node (block executor, network
//! stack, consensus, engine API, blockchain orchestrator) is modelled as
//! an `async_trait` service trait. There is no actor
//! framework, no `ActorRef`, no mailbox — the runtime is just `async` +
//! `tokio::sync::mpsc` + `tokio::sync::broadcast` + `tokio::sync::oneshot`,
//! which is what reth and polkadot-sdk do.
//!
//! ## Layering
//!
//! Sits in **Layer 1 (runtime)**. Depends only on:
//!
//! - `neo-primitives` (Layer 0) — for `UInt256`.
//! - `neo-payloads` (Layer 1) — for `Block` and `Transaction`.
//! - `neo-ledger-types` (Layer 1) — placeholder for future protocol
//!   types carried in events.
//! - `tokio`, `async-trait`, `futures`, `parking_lot`, `thiserror`,
//!   `tracing` — external async / utility crates.
//!
//! Concrete service implementations live in their respective domain
//! crates (e.g. `neo-blockchain` owns the blockchain service
//! implementing [`BlockExecutor`]). This crate owns the service
//! *traits*, the [`BlockchainHandle`] command/event channel, and the
//! [`ServiceError`] vocabulary. The concrete node **composition root**
//! — the single owner of all wired services that the `neo-node` binary
//! constructs at startup — lives in `neo-system` (`neo_system::Node`),
//! which depends on this crate for the trait vocabulary.
//!
//! ## Re-export index
//!
//! | Item | Path | Purpose |
//! |------|------|---------|
//! | Service trait base | [`Service`] | `Send + Sync + Debug + 'static` marker |
//! | Block executor | [`BlockExecutor`] | Execute / validate blocks |
//! | Network | [`NetworkService`] | P2P networking |
//! | Consensus | [`ConsensusService`] | dBFT loop |
//! | Engine | [`NeoEngine`] | Engine API |
//! | Tx hash | [`TxHash`] | `UInt256` alias |
//! | Blockchain handle | [`BlockchainHandle`] | Command / event channel |
//! | Blockchain command | [`BlockchainCommand`] | Per-request command enum |
//! | Blockchain event | [`BlockchainEvent`] | Per-event broadcast enum |
//! | Service error | [`ServiceError`] | Cross-service error vocabulary |
//! | Service result | [`ServiceResult`] | `Result<T, ServiceError>` alias |
//! | Outcome types | [`ExecutionOutcome`], [`ExecutionPayload`], [`ValidationResult`], [`NetworkEvent`] | Service return types |
//!
//! ## Quick start
//!
//! Each subsystem implements a service trait from this crate; the
//! concrete node composition that wires them together lives in
//! `neo-system`.
//!
//! ```no_run
//! use std::sync::Arc;
//! use async_trait::async_trait;
//! use neo_runtime::{BlockExecutor, BlockchainHandle, ExecutionOutcome, Service, ServiceError};
//! use neo_payloads::Block;
//!
//! #[derive(Debug)]
//! struct StubExecutor;
//! impl Service for StubExecutor {}
//! #[async_trait]
//! impl BlockExecutor for StubExecutor {
//!     async fn execute(&self, _block: &Block) -> Result<ExecutionOutcome, ServiceError> {
//!         Ok(ExecutionOutcome::default())
//!     }
//!     async fn validate(&self, _block: &Block) -> Result<(), ServiceError> {
//!         Ok(())
//!     }
//! }
//!
//! # async fn run() -> Result<(), ServiceError> {
//! // Services are stored and called as trait objects — no ActorRef, no mailbox.
//! let executor: Arc<dyn BlockExecutor> = Arc::new(StubExecutor);
//! let _outcome = executor.execute(&Block::new()).await?;
//!
//! // The blockchain orchestrator is exposed as a command/event channel handle.
//! let (_blockchain, _rx) = BlockchainHandle::with_capacity();
//! # Ok(()) }
//! ```

#![doc(html_root_url = "https://docs.rs/neo-runtime/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod blockchain;
pub mod errors;
pub mod outcome;
pub mod services;

// Re-exports for the public surface of the crate.
//
// Everything the spec promises at the top level of `neo_runtime` is
// exported here so the docstring "use neo_runtime::BlockExecutor"
// import path resolves.
pub use blockchain::{
    BlockchainCommand, BlockchainEvent, BlockchainHandle, DEFAULT_COMMAND_CAPACITY,
    DEFAULT_EVENT_CAPACITY,
};
pub use errors::{ServiceError, ServiceResult};
pub use outcome::{ExecutionOutcome, ExecutionPayload, NetworkEvent, ValidationResult};
pub use services::{
    BlockExecutor, ConsensusService, NeoEngine, NetworkService, Service, TxHash,
};
