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
//! - `mempool_facade`: Minimal mempool adapter boundary used by the service.
//! - `service`: Service loops, handles, lifecycle helpers, and command
//!   processing.
//! - `service_context`: blockchain service context traits.

pub(crate) mod command;
pub(crate) mod handle;
pub(crate) mod internal;
mod mempool_facade;
mod service;
pub(crate) mod service_context;

pub use mempool_facade::MempoolLike;
pub use service::BlockchainService;
