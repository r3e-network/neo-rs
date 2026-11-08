//! Contract-based transaction batching system
//!
//! This module implements Solana-inspired smart batching that groups transactions
//! by target contract address to improve parallelism and throughput.
//!
//! # Design Philosophy
//!
//! Based on Brian's research showing **+20-30% TPS increase** from contract-based
//! batching, this system:
//!
//! - **Static Analysis Only**: Extract contract addresses without VM execution
//! - **Greedy Packing**: O(n) algorithm avoiding NP-complete bin packing problem
//! - **Contract Prioritization**: Identified contracts grouped before defaults
//! - **Gas Constraints**: Strict per-batch gas limits enforced
//!
//! ## Architecture Overview
//!
//! ```mermaid
//! graph TB
//!     A[Pending Transactions] --> B{Extract Contract Hash}
//!     B -->|Known Contract| C[Contract Queue]
//!     B -->|Unknown/Default| D[Default Queue]
//!     C --> E[Greedy Pack Algorithm]
//!     D --> E
//!     E --> F[Batches for Parallel Execution]
//! ```
//!
//! # Usage Example
//!
//! ```rust,ignore
//! use neo_tee::mempool::batcher::{ContractBatchScheduler, ExecBatch};
//!
//! // Create scheduler (or reuse singleton)
//! let scheduler = ContractBatchScheduler::new();
//!
//! // Get transactions from mempool
//! let mempool_entries: Vec<TeeMempoolEntry> = /* ... */;
//!
//! // Schedule into batches
//! let batches = scheduler.schedule(&mempool_entries);
//!
//! // Each batch is optimized for parallel execution
//! for batch in batches {
//!     assert!(batch.total_count <= 100); // Default max batch size
//!     assert!(batch.total_gas <= 50_000_000); // Gas limit respected
//! }
//! ```
//!
//! ## Performance Characteristics
//!
//! - **Time Complexity**: O(n) where n = number of pending transactions
//! - **Space Complexity**: O(n) for storage of all transactions
//! - **Zero VM Execution**: Pure static bytecode analysis
//! - **Lock-Free Reads**: RwLock enables concurrent reads
//!
//! ## Key Components
//!
//! - [`ContractBatchScheduler`]: Main scheduling engine
//! - [`ExecBatch`]: Batch container with gas/count metadata
//! - [`SchedulingMetrics`]: Visibility into batching efficiency
//!
//! ## Implementation Notes
//!
//! The implementation uses heuristics to identify contract targets:
//!
//! 1. **SYSCALL Detection**: Scans for opcode 0x41 (4-byte descriptor follows)
//! 2. **CALLT Detection**: Identifies cross-contract call tokens (opcode 0x4E)
//! 3. **Script Analysis**: Conservative instruction boundary detection
//!
//! In production deployments, maintain a registry of known contract addresses
//! to further improve classification accuracy.

pub mod contract_batcher;

pub use contract_batcher::{ContractBatchScheduler, ExecBatch, SchedulingMetrics};
