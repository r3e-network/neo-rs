//! # neo-node::process
//!
//! Process-wide ownership and lifecycle primitives for node binaries.
//!
//! ## Boundary
//!
//! This module owns application-process coordination. It does not define Neo
//! protocol behavior, database transaction semantics, or service policy.
//!
//! ## Contents
//!
//! - `lifecycle_lock`: exclusive ownership of one local node data directory.

mod lifecycle_lock;

pub use lifecycle_lock::{NodeLifecycleLock, NodeLifecycleLockError};
