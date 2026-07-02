//! # neo-node::node::tasks
//!
//! Task supervision, shutdown wiring, and background-service handles.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `metrics`: bounded-label Prometheus task supervision metrics.
//! - `supervision`: essential/normal daemon task spawning and shutdown policy.

mod metrics;
mod supervision;

pub(in crate::node) use metrics::render_prometheus;
#[cfg(test)]
pub(in crate::node) use metrics::reset_for_tests as reset_metrics_for_tests;
pub(in crate::node) use supervision::{TaskKind, spawn_daemon_task, spawn_daemon_task_result};
