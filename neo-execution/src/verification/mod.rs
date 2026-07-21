//! # Witness verification support
//!
//! This module owns bounded, immutable acceleration artifacts used while the
//! canonical NeoVM verification script still executes in full.
//!
//! ## Boundary
//!
//! Verification support may cache exact state-independent cryptographic
//! outcomes. It does not authorize a witness, publish state, change gas or
//! fault behavior, or introduce another VM value model.
//!
//! ## Contents
//!
//! - Exact-input P-256 outcomes for canonical standard witness scripts.
//! - Per-cache canonical consumption, hit, and fallback metrics.

mod preverified_signatures;

pub use preverified_signatures::{
    PreverifiedSignatureCache, PreverifiedSignatureCacheMetricsSnapshot,
    preverify_standard_witness_signatures,
};
