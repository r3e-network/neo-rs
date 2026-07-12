//! # neo-blockchain::state_root
//!
//! Signed StateRoot consensus and witness verification.
//!
//! ## Boundary
//!
//! This module belongs to `neo-blockchain`. It coordinates StateValidator
//! signatures over state roots but does not own trie persistence, consensus
//! service scheduling, or node composition.
//!
//! ## Contents
//!
//! - `consensus`: StateValidator vote signing, collection, and aggregation.
//! - `verification`: designated-validator lookup and witness verification.

pub mod consensus;
pub mod verification;
