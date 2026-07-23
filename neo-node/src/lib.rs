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
//! - `process`: process-wide ownership and lifecycle primitives.

#![doc(html_root_url = "https://docs.rs/neo-node/0.11.0")]

mod process;

pub use process::{NodeLifecycleLock, NodeLifecycleLockError};
