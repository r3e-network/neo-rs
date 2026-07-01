//! # neo-tee::mempool
//!
//! TEE-facing mempool request helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-tee`. This adapter crate owns TEE integration
//! and must not define protocol bytes, consensus rules, or storage semantics.
//!
//! ## Contents
//!
//! - `fair_ordering`: fair-ordering mempool policy.
//! - `tee_mempool`: TEE-backed mempool facade.

// `fair_ordering` and `tee_mempool` are crate-visible (not just module-private)
// so the feature-gated `nitro` backend can reference `OrderingProof`,
// `FairOrderingPolicy` variants, and the sequencer types it reuses verbatim.
pub(crate) mod fair_ordering;
pub(crate) mod tee_mempool;

pub use fair_ordering::FairOrderingPolicy;
pub use tee_mempool::{TeeMempool, TeeMempoolConfig};
