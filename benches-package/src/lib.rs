//! # Workspace benchmark support
//!
//! Shared deterministic inputs, runners, and evidence schemas for storage and
//! execution benchmarks.
//!
//! ## Boundary
//!
//! This crate is benchmark tooling. It may drive production crates but does
//! not participate in node runtime composition or protocol authority.
//!
//! ## Contents
//!
//! - Append-pack persistence campaigns.
//! - Durable MDBX persistence campaigns.
//! - Backend-neutral storage workload generation.

pub mod append_benchmark;
pub mod mdbx_benchmark;
pub mod paritydb_benchmark;
pub mod storage_workload;
