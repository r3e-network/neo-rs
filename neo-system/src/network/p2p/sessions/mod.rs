//! Per-peer session bookkeeping for the P2P task manager.
//!
//! Mirrors `Neo.Network.P2P.TaskSession`. Declared so that
//! `crate::sessions::task_session::TaskSession` (used by `completion_flow` and
//! `actors::task_manager`) resolves.

/// Per-peer requested-inventory/index tracking state.
pub mod task_session;

pub use task_session::TaskSession;
