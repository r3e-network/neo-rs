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
//! ## Example
//!
//! ```rust,ignore
//! use neo_telemetry::{init, TelemetryConfig, LoggingConfig};
//!
//! // Initialize with default configuration
//! let config = TelemetryAndLoggingConfig::default();
//! let handle = init(&config)?;
//!
//! // Record metrics
//! if let Some(ref metrics) = handle.metrics {
//!     metrics.block_height.set(1000);
//! }
//! ```

mod config;
mod error;
mod health;
mod logging;
mod metrics;
mod node_health;
mod node_logging;
mod node_metrics;
mod system;

// Public exports - Config
pub use config::{LogFormat, LoggingConfig, TelemetryAndLoggingConfig, TelemetryConfig};

// Public exports - Error
pub use error::{TelemetryError, TelemetryResult};

// Public exports - Basic Health
pub use health::{ComponentHealth, HealthCheck, HealthCheckFn, HealthStatus};

// Public exports - Basic Logging
pub use logging::init_logging;

// Public exports - Basic Metrics
pub use metrics::{Metrics, MetricsServer};

// Public exports - Node-specific (merged from neo-node)
pub use node_health::{HealthState, NodeHealthServer, DEFAULT_MAX_HEADER_LAG};
pub use node_logging::{init_node_logging, LoggingGuard};
pub use node_metrics::{
    gather_prometheus, update_node_metrics, BlockMetrics, MempoolMetrics, NetworkMetrics,
    NodeMetrics, StateRootMetrics, StorageMetrics,
};

// Public exports - System
pub use system::SystemMonitor;

/// Initialize the complete telemetry and logging stack
///
/// This function initializes both logging and metrics collection.
/// Use this as the single entry point for telemetry in your application.
///
/// # Example
///
/// ```rust,ignore
/// use neo_telemetry::{init, TelemetryAndLoggingConfig};
///
/// let config = TelemetryAndLoggingConfig {
///     logging: LoggingConfig {
///         level: "info".to_string(),
///         format: LogFormat::Json,
///         ..Default::default()
///     },
///     telemetry: TelemetryConfig {
///         metrics_enabled: true,
///         ..Default::default()
///     },
/// };
///
/// let handle = init(&config)?;
/// ```
pub fn init(config: &TelemetryAndLoggingConfig) -> TelemetryResult<TelemetryHandle> {
    // Initialize logging
    init_logging(&config.logging)?;

    // Initialize metrics if enabled
    let metrics = if config.telemetry.metrics_enabled {
        Some(Metrics::new())
    } else {
        None
    };

    // Initialize system monitor
    let system_monitor = SystemMonitor::new();

    tracing::info!("Telemetry initialized successfully");

    Ok(TelemetryHandle {
        metrics,
        system_monitor,
        config: config.clone(),
    })
}

/// Initialize with node-specific logging (includes file output, daemon mode support)
///
/// This is the preferred initialization method for neo-node.
pub fn init_for_node(
    config: &TelemetryAndLoggingConfig,
    daemon_mode: bool,
) -> TelemetryResult<(TelemetryHandle, LoggingGuard)> {
    // Initialize node logging (with file support)
    let guard = init_node_logging(&config.logging, daemon_mode)?;

    // Initialize metrics if enabled
    let metrics = if config.telemetry.metrics_enabled {
        Some(Metrics::new())
    } else {
        None
    };

    let system_monitor = SystemMonitor::new();

    tracing::info!(
        metrics_enabled = config.telemetry.metrics_enabled,
        daemon_mode = daemon_mode,
        "Node telemetry initialized"
    );

    Ok((
        TelemetryHandle {
            metrics,
            system_monitor,
            config: config.clone(),
        },
        guard,
    ))
}

/// Handle to telemetry resources
///
/// This handle holds references to all telemetry components.
/// Keep it alive for the duration of your application.
pub struct TelemetryHandle {
    /// Prometheus metrics (if enabled)
    pub metrics: Option<Metrics>,

    /// System resource monitor
    pub system_monitor: SystemMonitor,

    /// Configuration (kept for reference)
    config: TelemetryAndLoggingConfig,
}

impl TelemetryHandle {
    /// Get a reference to the configuration
    pub fn config(&self) -> &TelemetryAndLoggingConfig {
        &self.config
    }

    /// Check if metrics are enabled
    pub fn metrics_enabled(&self) -> bool {
        self.metrics.is_some()
    }

    /// Get metrics registry (panics if metrics not enabled)
    pub fn metrics(&self) -> &Metrics {
        self.metrics.as_ref().expect("metrics not enabled")
    }

    /// Get metrics registry optionally
    pub fn metrics_opt(&self) -> Option<&Metrics> {
        self.metrics.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_handle() {
        let handle = TelemetryHandle {
            metrics: None,
            system_monitor: SystemMonitor::new(),
            config: TelemetryAndLoggingConfig::default(),
        };

        assert!(!handle.metrics_enabled());
        assert!(handle.metrics_opt().is_none());
    }

    #[test]
    fn test_telemetry_handle_with_metrics() {
        let handle = TelemetryHandle {
            metrics: Some(Metrics::new()),
            system_monitor: SystemMonitor::new(),
            config: TelemetryAndLoggingConfig::default(),
        };

        assert!(handle.metrics_enabled());
        assert!(handle.metrics_opt().is_some());
    }
}
