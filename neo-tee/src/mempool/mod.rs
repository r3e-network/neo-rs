//! TEE-protected mempool with fair transaction ordering
//!
//! This module implements a fair ordering policy to prevent MEV attacks.
//!
//! ## Submodules
//!
//! - **batcher**: Contract-based transaction batching (Solana-inspired)
//! - **fair_ordering**: Fair transaction ordering for MEV prevention
//! - **tee_mempool**: Core TEE mempool implementation

mod batcher;
mod fair_ordering;
mod tee_mempool;

pub use batcher::{ContractBatchScheduler, ExecBatch, SchedulingMetrics};
pub use fair_ordering::FairOrderingPolicy;
pub use tee_mempool::{TeeMempool, TeeMempoolConfig};
