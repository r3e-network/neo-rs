//! # Neo Telemetry
//!
//! Production observability stack for Neo N3 blockchain node.
//!
//! **IMPORTANT**: This crate provides the **production deployment** telemetry stack.
//! For internal metrics collection within neo-core components (without external dependencies),
//! use `neo_core::telemetry` instead.
//!
//! ## When to use this crate
//!
//! - **Production deployment**: HTTP metrics endpoint for Prometheus scraping
//! - **System monitoring**: CPU, memory, disk usage metrics
//! - **Health checks**: Liveness and readiness probes for Kubernetes
//! - **Logging configuration**: Structured logging with JSON or text output
//!
//! ## When to use neo-core::telemetry
//!
//! - **Internal metrics**: Recording blockchain metrics within neo-core components
//! - **No external dependencies**: When you need lightweight metric collection
//! - **Snapshot export**: Getting point-in-time metric snapshots
//! - **Timer utilities**: Measuring operation durations
//!
//! ## Features
//!
//! - **Logging**: JSON or text format with configurable levels
//! - **Metrics**: Block height, peer count, memory usage, etc. (Prometheus native)
//! - **Health**: Liveness and readiness probes

mod error;
mod health;
mod logging;
mod metrics;
mod system;

pub use error::{TelemetryError, TelemetryResult};
pub use health::{HealthCheck, HealthStatus};
pub use logging::{init_logging, LogConfig};
pub use metrics::{Metrics, MetricsServer};
pub use system::SystemMonitor;

/// Initialize the complete telemetry stack
pub fn init(config: &neo_config::TelemetrySettings) -> TelemetryResult<TelemetryHandle> {
    let log_config = LogConfig::default();
    init_logging(&log_config)?;

    let metrics = if config.metrics_enabled {
        Some(Metrics::new())
    } else {
        None
    };

    let system_monitor = SystemMonitor::new();

    Ok(TelemetryHandle {
        metrics,
        system_monitor,
    })
}

/// Handle to telemetry resources
pub struct TelemetryHandle {
    /// Prometheus metrics (if enabled)
    pub metrics: Option<Metrics>,

    /// System resource monitor
    pub system_monitor: SystemMonitor,
}
