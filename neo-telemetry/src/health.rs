//! Health check functionality

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

/// Health status of a component
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    /// Component is healthy
    Healthy,
    /// Component is degraded but functional
    Degraded,
    /// Component is unhealthy
    Unhealthy,
}

impl HealthStatus {
    /// Check if status indicates healthy
    pub fn is_healthy(&self) -> bool {
        matches!(self, HealthStatus::Healthy)
    }

    /// Check if status is not unhealthy
    pub fn is_ok(&self) -> bool {
        !matches!(self, HealthStatus::Unhealthy)
    }
}

/// Health check result for a single component
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    /// Component name
    pub name: String,

    /// Health status
    pub status: HealthStatus,

    /// Optional message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Check duration in milliseconds
    pub duration_ms: u64,
}

/// Overall health check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    /// Overall status
    pub status: HealthStatus,

    /// Individual component results
    pub components: HashMap<String, ComponentHealth>,

    /// Timestamp
    pub timestamp: u64,
}

/// Health check manager
pub struct HealthCheck {
    checks: Vec<Box<dyn HealthCheckFn>>,
}

/// Trait for health check functions
pub trait HealthCheckFn: Send + Sync {
    /// Get component name
    fn name(&self) -> &str;

    /// Perform health check
    fn check(&self) -> ComponentHealth;
}

impl HealthCheck {
    /// Create a new health check manager
    pub fn new() -> Self {
        Self { checks: Vec::new() }
    }

    /// Register a health check
    pub fn register<C: HealthCheckFn + 'static>(&mut self, check: C) {
        self.checks.push(Box::new(check));
    }

    /// Run all health checks
    pub fn check_all(&self) -> HealthCheckResult {
        let mut components = HashMap::new();
        let mut overall_status = HealthStatus::Healthy;

        for check in &self.checks {
            let result = check.check();

            // Update overall status
            match (&overall_status, &result.status) {
                (HealthStatus::Healthy, HealthStatus::Degraded) => {
                    overall_status = HealthStatus::Degraded;
                }
                (_, HealthStatus::Unhealthy) => {
                    overall_status = HealthStatus::Unhealthy;
                }
                _ => {}
            }

            components.insert(check.name().to_string(), result);
        }

        HealthCheckResult {
            status: overall_status,
            components,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// Check liveness (basic check that service is running)
    pub fn liveness(&self) -> bool {
        true // If we can run this code, we're alive
    }

    /// Check readiness (can service handle requests)
    pub fn readiness(&self) -> bool {
        let result = self.check_all();
        result.status.is_ok()
    }
}

impl Default for HealthCheck {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple function-based health check
///
/// This type allows creating custom health checks from closures.
/// It is a public API intended for consumers who want to add
/// custom health checks to their Neo node monitoring.
///
/// # Example
///
/// ```rust,ignore
/// use neo_telemetry::health::{FnHealthCheck, HealthStatus};
///
/// let db_check = FnHealthCheck::new("database", || {
///     // Check database connectivity
///     (HealthStatus::Healthy, Some("Connected".to_string()))
/// });
/// ```
#[allow(dead_code)]
pub struct FnHealthCheck<F>
where
    F: Fn() -> (HealthStatus, Option<String>) + Send + Sync,
{
    name: String,
    check_fn: F,
}

#[allow(dead_code)]
impl<F> FnHealthCheck<F>
where
    F: Fn() -> (HealthStatus, Option<String>) + Send + Sync,
{
    /// Create a new function-based health check
    pub fn new(name: impl Into<String>, check_fn: F) -> Self {
        Self {
            name: name.into(),
            check_fn,
        }
    }
}

impl<F> HealthCheckFn for FnHealthCheck<F>
where
    F: Fn() -> (HealthStatus, Option<String>) + Send + Sync,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn check(&self) -> ComponentHealth {
        let start = Instant::now();
        let (status, message) = (self.check_fn)();
        let duration_ms = start.elapsed().as_millis() as u64;

        ComponentHealth {
            name: self.name.clone(),
            status,
            message,
            duration_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status() {
        assert!(HealthStatus::Healthy.is_healthy());
        assert!(HealthStatus::Healthy.is_ok());
        assert!(!HealthStatus::Degraded.is_healthy());
        assert!(HealthStatus::Degraded.is_ok());
        assert!(!HealthStatus::Unhealthy.is_ok());
    }

    #[test]
    fn test_health_check() {
        let mut health = HealthCheck::new();

        health.register(FnHealthCheck::new("test", || {
            (HealthStatus::Healthy, None)
        }));

        let result = health.check_all();
        assert_eq!(result.status, HealthStatus::Healthy);
        assert!(result.components.contains_key("test"));
    }

    #[test]
    fn test_degraded_propagation() {
        let mut health = HealthCheck::new();

        health.register(FnHealthCheck::new("healthy", || {
            (HealthStatus::Healthy, None)
        }));

        health.register(FnHealthCheck::new("degraded", || {
            (HealthStatus::Degraded, Some("Slow response".to_string()))
        }));

        let result = health.check_all();
        assert_eq!(result.status, HealthStatus::Degraded);
    }
}
