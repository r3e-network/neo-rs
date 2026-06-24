//! # neo-runtime
//!
//! Reth-style async service architecture for the Neo node.
//!
//! This crate is the **service pattern** for the rest of the workspace.
//! Every long-running component of a Neo node (block executor, network
//! stack, consensus, engine API, blockchain orchestrator) is modelled as
//! an `async_trait` service trait. The runtime surface is plain `async` plus
//! the channel primitives used by the concrete service crates
//! (`tokio::sync::mpsc`, `tokio::sync::broadcast`, and
//! `tokio::sync::oneshot`).
//!
//! ## Layering
//!
//! Sits in **Layer 3 (Domain services)**. It defines the shared
//! service vocabulary used by higher node-service and composition
//! crates, while depending only on lower protocol / foundation crates:
//!
//! - `neo-primitives` (Layer 0) — for `UInt256`.
//! - `neo-payloads` (Layer 2) — for `Block` and `Transaction`.
//! - `tokio`, `async-trait`, `serde`, `thiserror` — external async /
//!   serialization / error crates.
//!
//! Concrete service implementations live in their respective domain and
//! node-service crates (for example `neo-blockchain` owns the blockchain
//! command loop, and `neo-network` owns the P2P service). This crate owns the
//! service *traits*, the [`BlockchainEvent`] broadcast type, and the
//! [`ServiceError`] vocabulary shared at service boundaries. The concrete
//! composition roots live above this layer (`neo-system` for embeddable
//! composition and `neo-node` for the runnable daemon).
//!
//! ## Crate shape
//!
//! `neo-runtime` is intentionally small and independent. Merging it into
//! `neo-system` would force lower service crates to depend upward on the
//! composition layer; merging it into `neo-blockchain` or `neo-network` would
//! make unrelated services depend on a concrete implementation crate just to
//! share trait vocabulary. Keeping this crate separate preserves an acyclic
//! boundary: service implementations depend on the shared contracts, and
//! composition crates depend on both.
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
//! | Blockchain event | [`BlockchainEvent`] | Per-event broadcast enum |
//! | Service error | [`ServiceError`] | Cross-service error vocabulary |
//! | Service result | [`ServiceResult`] | `Result<T, ServiceError>` alias |
//! | Outcome types | [`ExecutionOutcome`], [`ExecutionPayload`], [`ValidationResult`], [`NetworkEvent`] | Service return types |
//!
//! ## Quick start
//!
//! Each subsystem implements a service trait from this crate; the
//! concrete node composition that wires them together lives above this crate.
//!
//! ```no_run
//! use std::sync::Arc;
//! use async_trait::async_trait;
//! use neo_runtime::{BlockExecutor, ExecutionOutcome, Service, ServiceError};
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
//! // Services are stored and called as trait objects.
//! let executor: Arc<dyn BlockExecutor> = Arc::new(StubExecutor);
//! let _outcome = executor.execute(&Block::new()).await?;
//! # Ok(()) }
//! ```

#![doc(html_root_url = "https://docs.rs/neo-runtime/0.8.0")]

pub mod blockchain;
pub mod error;
pub mod outcome;
pub mod services;
pub mod sync_metrics;

// Re-exports for the public surface of the crate.
//
// Everything the spec promises at the top level of `neo_runtime` is
// exported here so the docstring "use neo_runtime::BlockExecutor"
// import path resolves.
pub use blockchain::{BlockchainEvent, DEFAULT_COMMAND_CAPACITY, DEFAULT_EVENT_CAPACITY};
pub use error::{ServiceError, ServiceResult};
pub use outcome::{ExecutionOutcome, ExecutionPayload, NetworkEvent, ValidationResult};
pub use services::{BlockExecutor, ConsensusService, NeoEngine, NetworkService, Service, TxHash};
