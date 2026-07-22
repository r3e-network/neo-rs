//! # neo-node
//!
//! Application-level process coordination shared by the Neo node binaries.
//!
//! ## Boundary
//!
//! This library owns process lifecycle mechanics for `neo-node`. It does not
//! define protocol behavior, storage formats, or persistence semantics.
//!
//! ## Contents
//!
//! - [`lifecycle_lock`]: exclusive ownership of one local node data directory.

#![doc(html_root_url = "https://docs.rs/neo-node/0.10.0")]

pub mod lifecycle_lock;

pub use lifecycle_lock::{NodeLifecycleLock, NodeLifecycleLockError};
