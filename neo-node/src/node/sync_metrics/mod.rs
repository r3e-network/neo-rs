//! # neo-node::node::sync_metrics
//!
//! Sync-speed counters, summaries, and operator-facing throughput status.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `families`: labelled metric-family rendering for lower-level sync stages.
//! - `render`: Prometheus text rendering for node sync metrics.
//! - `writer`: small Prometheus label/value writers.

mod families;
mod render;
mod writer;

pub use render::render_prometheus;
