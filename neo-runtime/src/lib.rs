//! # neo-runtime
//!
//! Reth-style async service architecture for the Neo node.
//!
//! This crate is the **service pattern** for the rest of the workspace.
//! Every long-running component of a Neo node (block executor, mempool,
//! network stack, consensus, engine API, blockchain orchestrator) is
//! modelled as an `async_trait` service and is *constructed* via the
//! [`NodeBuilder`]. There is no actor framework, no `ActorRef`, no
//! mailbox — the runtime is just `async` + `tokio::sync::mpsc` +
//! `tokio::sync::broadcast` + `tokio::sync::oneshot`, which is what reth
//! and polkadot-sdk do.
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
//! crates (e.g. `neo-blockchain` will own a `BlockExecutorService`
//! implementing [`BlockExecutor`]). This crate owns the *traits* and
//! the [`Node`] composition root only.
//!
//! ## Re-export index
//!
//! | Item | Path | Purpose |
//! |------|------|---------|
//! | Service trait base | [`Service`] | `Send + Sync + Debug + 'static` marker |
//! | Block executor | [`BlockExecutor`] | Execute / validate blocks |
//! | Mempool | [`MempoolService`] | Transaction pool |
//! | Network | [`NetworkService`] | P2P networking |
//! | Consensus | [`ConsensusService`] | dBFT loop |
//! | Engine | [`NeoEngine`] | Engine API |
//! | Tx hash | [`TxHash`] | `UInt256` alias |
//! | Blockchain handle | [`BlockchainHandle`] | Command / event channel |
//! | Blockchain command | [`BlockchainCommand`] | Per-request command enum |
//! | Blockchain event | [`BlockchainEvent`] | Per-event broadcast enum |
//! | Service error | [`ServiceError`] | Cross-service error vocabulary |
//! | Service result | [`ServiceResult`] | `Result<T, ServiceError>` alias |
//! | Node | [`Node`] | Composition of all services |
//! | Node builder | [`NodeBuilder`] | Fluent builder for [`Node`] |
//! | Outcome types | [`ExecutionOutcome`], [`ExecutionPayload`], [`ValidationResult`], [`NetworkEvent`] | Service return types |
//!
//! ## Quick start
//!
//! ```no_run
//! use std::sync::Arc;
//! use async_trait::async_trait;
//! use neo_runtime::{
//!     BlockExecutor, BlockchainHandle, MempoolService, NeoEngine, NetworkService,
//!     Node, Service, ServiceError, ExecutionOutcome,
//! };
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
//! // ... build the other services the same way ...
//! # #[derive(Debug)]
//! # struct StubMempool;
//! # impl Service for StubMempool {}
//! # #[async_trait]
//! # impl MempoolService for StubMempool {
//! #     async fn add_transaction(&self, _tx: neo_payloads::Transaction) -> Result<neo_runtime::TxHash, ServiceError> { Ok(neo_primitives::UInt256::default()) }
//! #     async fn get_transactions(&self, _max: usize) -> Result<Vec<neo_payloads::Transaction>, ServiceError> { Ok(Vec::new()) }
//! #     async fn remove_transaction(&self, _hash: &neo_primitives::UInt256) -> Result<(), ServiceError> { Ok(()) }
//! #     async fn count(&self) -> usize { 0 }
//! # }
//! # #[derive(Debug)]
//! # struct StubNetwork;
//! # impl Service for StubNetwork {}
//! # #[async_trait]
//! # impl NetworkService for StubNetwork {
//! #     async fn broadcast_block(&self, _block: &Block) -> Result<(), ServiceError> { Ok(()) }
//! #     async fn broadcast_transaction(&self, _tx: &neo_payloads::Transaction) -> Result<(), ServiceError> { Ok(()) }
//! #     async fn peer_count(&self) -> usize { 0 }
//! #     fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<neo_runtime::NetworkEvent> {
//! #         let (_tx, rx) = tokio::sync::broadcast::channel(1); rx
//! #     }
//! # }
//! # #[derive(Debug)]
//! # struct StubConsensus;
//! # impl Service for StubConsensus {}
//! # #[async_trait]
//! # impl neo_runtime::ConsensusService for StubConsensus {
//! #     async fn start(&self) -> Result<(), ServiceError> { Ok(()) }
//! #     async fn stop(&self) -> Result<(), ServiceError> { Ok(()) }
//! #     async fn is_running(&self) -> bool { false }
//! # }
//! # #[derive(Debug)]
//! # struct StubEngine;
//! # impl Service for StubEngine {}
//! # #[async_trait]
//! # impl NeoEngine for StubEngine {
//! #     async fn execute_block(&self, _block: &Block) -> Result<neo_runtime::ExecutionPayload, ServiceError> { Ok(neo_runtime::ExecutionPayload::default()) }
//! #     async fn validate_block(&self, _block: &Block) -> Result<neo_runtime::ValidationResult, ServiceError> { Ok(neo_runtime::ValidationResult::ok()) }
//! # }
//!
//! # async fn run() -> Result<(), ServiceError> {
//! let (blockchain, _rx) = BlockchainHandle::with_capacity();
//! let node = Node::builder()
//!     .with_block_executor(Arc::new(StubExecutor))
//!     .with_mempool(Arc::new(StubMempool))
//!     .with_network(Arc::new(StubNetwork))
//!     .with_consensus(Arc::new(StubConsensus))
//!     .with_engine(Arc::new(StubEngine))
//!     .with_blockchain(blockchain)
//!     .build()?;
//!
//! // Call services through the trait object — no ActorRef, no mailbox.
//! let _ = node.mempool.count().await;
//! # Ok(()) }
//! ```

#![doc(html_root_url = "https://docs.rs/neo-runtime/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod blockchain;
pub mod errors;
pub mod node;
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
pub use node::{Node, NodeBuilder};
pub use outcome::{ExecutionOutcome, ExecutionPayload, NetworkEvent, ValidationResult};
pub use services::{
    BlockExecutor, ConsensusService, MempoolService, NeoEngine, NetworkService, Service, TxHash,
};
