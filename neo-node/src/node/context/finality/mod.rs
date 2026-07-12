//! # Finalized Read Projections
//!
//! ApplicationLogs and TokensTracker are optional, non-consensus projections.
//! They consume the same acknowledged post-canonical stream while critical
//! StateService, index, and archive work remains in the durability hooks.
//!
//! ## Boundary
//!
//! This application module selects concrete optional projections. It does not
//! own the generic stream contract, canonical persistence, or protocol bytes.
//!
//! ## Contents
//!
//! - `projections`: concrete ApplicationLogs and TokensTracker consumer.

mod projections;

pub(in crate::node) use projections::FinalizedProjectionConsumer;
