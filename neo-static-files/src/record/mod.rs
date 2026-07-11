//! # Static records
//!
//! ## Boundary
//!
//! Records carry one finalized height and opaque key/value rows. Neo Ledger
//! key selection and serialization belong to higher-level adapters.
//!
//! ## Contents
//!
//! - `types`: Finalized-height record and row value objects.

mod types;

pub use types::{StaticRecord, StaticRow};
