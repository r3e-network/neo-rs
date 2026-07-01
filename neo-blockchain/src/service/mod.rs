//! # neo-blockchain::service
//!
//! Service loops, handles, lifecycle helpers, and command processing.
//!
//! ## Boundary
//!
//! This module belongs to `neo-blockchain`. This node-service crate owns the
//! concrete block-import path and must not depend upward on composition, RPC,
//! GUI, or binaries.
//!
//! ## Contents
//!
//! - `command`: Command records sent into the service loop.
//! - `handle`: Typed handle used to interact with the service task.
//! - `internal`: service-internal queues and pending-block state.
//! - `service`: Service loops, handles, lifecycle helpers, and command
//!   processing.
//! - `service_context`: blockchain service context traits.

pub mod command;
pub mod handle;
pub mod internal;
pub mod service;
pub mod service_context;

pub use service::{BlockchainService, MempoolLike};
