//! Telemetry (metrics + health) configuration.

use serde::{Deserialize, Serialize};

/// Telemetry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySettings {
    /// Enable metrics collection
    #[serde(default)]
    pub metrics_enabled: bool,

    /// Metrics endpoint address
    #[serde(default = "default_metrics_address")]
    pub metrics_address: String,

    /// Metrics port
    #[serde(default = "default_metrics_port")]
    pub metrics_port: u16,

    /// Enable health check endpoint
    #[serde(default = "default_health_enabled")]
    pub health_enabled: bool,
}

fn default_metrics_address() -> String {
    "127.0.0.1".to_string()
}

const fn default_metrics_port() -> u16 {
    9090
}

const fn default_health_enabled() -> bool {
    true
}

impl Default for TelemetrySettings {
    fn default() -> Self {
        Self {
            metrics_enabled: false,
            metrics_address: default_metrics_address(),
            metrics_port: default_metrics_port(),
            health_enabled: default_health_enabled(),
        }
    }
}
