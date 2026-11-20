//! Per-peer session bookkeeping for the P2P task manager.
//!
//! Mirrors `Neo.Network.P2P.TaskSession`. This is the single home of the type;
//! the task manager and its lifecycle helpers import
//! `crate::p2p::sessions::task_session::TaskSession`.

/// Per-peer requested-inventory/index tracking state.
pub mod task_session;

pub use task_session::TaskSession;
