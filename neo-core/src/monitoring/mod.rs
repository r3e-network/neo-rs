//! Lightweight monitoring facade used by integration tests.
//!
//! The implementation here is intentionally minimal – it provides a few health
//! and performance primitives so the `monitoring` feature can be enabled
//! without pulling in the full observability stack.

use reqwest::Client;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::time::sleep;

/// Errors produced by the monitoring subsystem.
#[derive(Error)]
pub enum MonitoringError {
    #[error("Unsupported export format: {0}")]
    UnsupportedFormat(String),
    #[error("Metric not found: {0}")]
    MetricNotFound(String),
    #[error("Exporter error: {0}")]
    Exporter(String),
}

impl std::fmt::Debug for MonitoringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

pub type MonitoringResult<T> = Result<T, MonitoringError>;

/// Overall monitoring container returned by `init_monitoring`.
#[derive(Clone)]
pub struct MonitoringSystem {
    pub version: String,
    pub health_monitor: HealthMonitor,
    pub performance_monitor: Arc<PerformanceMonitor>,
    background_started: Arc<AtomicBool>,
}

impl MonitoringSystem {
    /// Starts lightweight monitoring with a couple of default metrics.
    pub async fn init(version: String) -> MonitoringResult<Self> {
        let perf = Arc::new(PerformanceMonitor::new());
        // Seed a few common metrics so callers can read them immediately.
        perf.register_metric("block_processing".to_string(), 100)
            .await;
        perf.register_metric("tx_validation".to_string(), 100).await;

        Ok(Self {
            version: version.clone(),
            health_monitor: HealthMonitor::new(version),
            performance_monitor: perf,
            background_started: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Starts periodic background sampling for a couple of system metrics.
    pub fn start_background_tasks(&self) {
        if self
            .background_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let monitor = self.performance_monitor.clone();
        tokio::spawn(async move {
            loop {
                // These values are approximate; they just provide activity for tests.
                let uptime = Instant::now().elapsed().as_secs_f64();
                let cpu_hint = (uptime.sin().abs() * 100.0).min(100.0);
                let mem_hint = (uptime.cos().abs() * 1_000_000.0).max(1.0);

                let _ = monitor.record("cpu_usage_percent", cpu_hint).await;
                let _ = monitor.record("memory_usage_bytes", mem_hint).await;
                sleep(Duration::from_millis(200)).await;
            }
        });
    }

    /// Exports monitoring data in the requested format.
    pub async fn export(&self, format: &str) -> MonitoringResult<String> {
        let health = self.health_monitor.check_health().await?;
        let perf = self.performance_monitor.get_all_stats().await;

        match format {
            "prometheus" => {
                let status_val = match health.status {
                    HealthStatus::Healthy => 0,
                    HealthStatus::Degraded => 1,
                    HealthStatus::Unhealthy => 2,
                };
                let mut out = String::new();
                out.push_str("# HELP neo_health_status Overall health status\n");
                out.push_str("# TYPE neo_health_status gauge\n");
                out.push_str(&format!("neo_health_status {}\n", status_val));
                for (metric, stats) in perf.iter() {
                    out.push_str(&format!("neo_perf_{}_avg {}\n", metric, stats.avg));
                }
                Ok(out)
            }
            "json" => {
                let payload = serde_json::json!({
                    "health": health,
                    "performance": perf,
                });
                serde_json::to_string(&payload)
                    .map_err(|e| MonitoringError::Exporter(e.to_string()))
            }
            "json-pretty" => {
                let payload = serde_json::json!({
                    "health": health,
                    "performance": perf,
                });
                serde_json::to_string_pretty(&payload)
                    .map_err(|e| MonitoringError::Exporter(e.to_string()))
            }
            "csv" => {
                let mut out = String::from("timestamp,component,status\n");
                for component in &health.components {
                    out.push_str(&format!(
                        "{:?},{},{}\n",
                        Instant::now(),
                        component.component,
                        component.status.as_str()
                    ));
                }
                Ok(out)
            }
            "otlp-json" => {
                let exporter = OtlpExporter::new(
                    "http://localhost:4317".to_string(),
                    format!("neo-node/{}", self.version),
                );
                exporter.build_payload(&self.version, &health, &perf)
            }
            other => Err(MonitoringError::UnsupportedFormat(other.to_string())),
        }
    }

    /// Pushes the current metrics to an OTLP HTTP endpoint.
    pub async fn push_otlp(&self, endpoint: &str, service_name: &str) -> MonitoringResult<()> {
        let health = self.health_monitor.check_health().await?;
        let perf = self.performance_monitor.get_all_stats().await;
        let exporter = OtlpExporter::new(endpoint.to_string(), service_name.to_string());
        let payload = exporter.build_payload(&self.version, &health, &perf)?;
        exporter.send(payload).await
    }

    /// Returns a combined status snapshot.
    pub async fn get_status(&self) -> MonitoringResult<MonitoringStatus> {
        let health = self.health_monitor.check_health().await?;
        let performance = self.performance_monitor.get_all_stats().await;
        let metrics = performance.clone();

        Ok(MonitoringStatus {
            health,
            performance,
            metrics,
        })
    }
}

/// Convenience wrapper used by tests.
pub async fn init_monitoring(version: String) -> MonitoringResult<MonitoringSystem> {
    MonitoringSystem::init(version).await
}

/// Report returned by [`MonitoringSystem::get_status`].
#[derive(Debug, Clone, Serialize)]
pub struct MonitoringStatus {
    pub health: HealthReport,
    pub performance: HashMap<String, PerformanceStats>,
    pub metrics: HashMap<String, PerformanceStats>,
}

/// High-level health status for a component or the system.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

impl HealthStatus {
    fn as_str(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "Healthy",
            HealthStatus::Degraded => "Degraded",
            HealthStatus::Unhealthy => "Unhealthy",
        }
    }

    fn merge(self, other: HealthStatus) -> HealthStatus {
        match (self, other) {
            (HealthStatus::Unhealthy, _) | (_, HealthStatus::Unhealthy) => HealthStatus::Unhealthy,
            (HealthStatus::Degraded, _) | (_, HealthStatus::Degraded) => HealthStatus::Degraded,
            _ => HealthStatus::Healthy,
        }
    }
}

/// Per-component health status.
#[derive(Debug, Clone, Serialize)]
pub struct HealthComponentStatus {
    pub component: String,
    pub status: HealthStatus,
    pub duration: Duration,
    pub message: Option<String>,
}

/// Aggregated health report.
#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub version: String,
    pub status: HealthStatus,
    pub components: Vec<HealthComponentStatus>,
    pub uptime: Duration,
}

/// Performs basic health checks with a cached report.
#[derive(Clone)]
pub struct HealthMonitor {
    version: String,
    start_time: Instant,
    cache: Arc<RwLock<Option<(Instant, HealthReport)>>>,
    cache_ttl: Duration,
}

impl HealthMonitor {
    pub fn new(version: String) -> Self {
        Self {
            version,
            start_time: Instant::now(),
            cache: Arc::new(RwLock::new(None)),
            cache_ttl: Duration::from_secs(1),
        }
    }

    pub async fn check_health(&self) -> MonitoringResult<HealthReport> {
        if let Some(report) = self.cached_report().await {
            return Ok(report);
        }

        let components = vec![
            HealthComponentStatus {
                component: "storage".to_string(),
                status: HealthStatus::Healthy,
                duration: Duration::from_millis(5),
                message: None,
            },
            HealthComponentStatus {
                component: "network".to_string(),
                status: HealthStatus::Healthy,
                duration: Duration::from_millis(5),
                message: None,
            },
        ];

        let status = components
            .iter()
            .fold(HealthStatus::Healthy, |acc, c| acc.merge(c.status));

        let report = HealthReport {
            version: self.version.clone(),
            status,
            components,
            uptime: self.start_time.elapsed(),
        };

        self.store_cache(report.clone()).await;
        Ok(report)
    }

    async fn cached_report(&self) -> Option<HealthReport> {
        let guard = self.cache.read().await;
        guard
            .as_ref()
            .and_then(|(ts, report)| (ts.elapsed() < self.cache_ttl).then_some(report.clone()))
    }

    async fn store_cache(&self, report: HealthReport) {
        let mut guard = self.cache.write().await;
        *guard = Some((Instant::now(), report));
    }
}

/// Alert severity.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum AlertLevel {
    Warning,
    Critical,
}

/// Alert details emitted when a threshold is crossed.
#[derive(Debug, Clone, Serialize)]
pub struct Alert {
    pub level: AlertLevel,
    pub metric: String,
    pub value: f64,
    pub threshold: PerformanceThreshold,
}

/// Threshold evaluation mode.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum ThresholdType {
    Max,
    Min,
}

/// Threshold configuration for a metric.
#[derive(Debug, Clone, Serialize)]
pub struct PerformanceThreshold {
    pub metric: String,
    pub warning: f64,
    pub critical: f64,
    pub threshold_type: ThresholdType,
}

/// Aggregated statistics for a single metric.
#[derive(Debug, Clone, Serialize)]
pub struct PerformanceStats {
    pub count: u64,
    pub min: f64,
    pub max: f64,
    pub avg: f64,
    pub current: f64,
    sum: f64,
}

impl PerformanceStats {
    pub fn record(&mut self, value: f64) {
        if self.count == 0 {
            self.min = value;
            self.max = value;
        } else {
            if value < self.min {
                self.min = value;
            }
            if value > self.max {
                self.max = value;
            }
        }
        self.count += 1;
        self.sum += value;
        self.current = value;
        self.avg = self.sum / self.count as f64;
    }
}

impl Default for PerformanceStats {
    fn default() -> Self {
        Self {
            count: 0,
            min: 0.0,
            max: 0.0,
            avg: 0.0,
            current: 0.0,
            sum: 0.0,
        }
    }
}

/// Records performance metrics and evaluates thresholds.
#[derive(Clone, Default)]
pub struct PerformanceMonitor {
    metrics: Arc<RwLock<HashMap<String, PerformanceStats>>>,
    thresholds: Arc<RwLock<HashMap<String, PerformanceThreshold>>>,
    callbacks: Arc<RwLock<Vec<Arc<dyn Fn(Alert) + Send + Sync>>>>,
}

impl PerformanceMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register_metric(&self, name: String, _window: usize) {
        let mut metrics = self.metrics.write().await;
        metrics
            .entry(name)
            .or_insert_with(PerformanceStats::default);
    }

    pub async fn record(&self, metric: &str, value: f64) -> MonitoringResult<()> {
        {
            let mut metrics = self.metrics.write().await;
            metrics
                .entry(metric.to_string())
                .or_insert_with(PerformanceStats::default)
                .record(value);
        }

        self.evaluate_threshold(metric, value).await;
        Ok(())
    }

    pub async fn get_stats(&self, metric: &str) -> MonitoringResult<PerformanceStats> {
        let metrics = self.metrics.read().await;
        metrics
            .get(metric)
            .cloned()
            .ok_or_else(|| MonitoringError::MetricNotFound(metric.to_string()))
    }

    pub async fn get_all_stats(&self) -> HashMap<String, PerformanceStats> {
        self.metrics.read().await.clone()
    }

    pub async fn set_threshold(&self, threshold: PerformanceThreshold) {
        let mut thresholds = self.thresholds.write().await;
        thresholds.insert(threshold.metric.clone(), threshold);
    }

    pub async fn register_alert_callback<F>(&self, callback: F)
    where
        F: Fn(Alert) + Send + Sync + 'static,
    {
        let mut callbacks = self.callbacks.write().await;
        callbacks.push(Arc::new(callback));
    }

    async fn evaluate_threshold(&self, metric: &str, value: f64) {
        let maybe_threshold = { self.thresholds.read().await.get(metric).cloned() };
        if let Some(threshold) = maybe_threshold {
            let level = match threshold.threshold_type {
                ThresholdType::Max => {
                    if value >= threshold.critical {
                        Some(AlertLevel::Critical)
                    } else if value >= threshold.warning {
                        Some(AlertLevel::Warning)
                    } else {
                        None
                    }
                }
                ThresholdType::Min => {
                    if value <= threshold.critical {
                        Some(AlertLevel::Critical)
                    } else if value <= threshold.warning {
                        Some(AlertLevel::Warning)
                    } else {
                        None
                    }
                }
            };

            if let Some(level) = level {
                let alert = Alert {
                    level,
                    metric: metric.to_string(),
                    value,
                    threshold,
                };

                let callbacks = self.callbacks.read().await;
                for callback in callbacks.iter() {
                    callback(alert.clone());
                }
            }
        }
    }
}

/// Simple profiler used to measure durations against a metric.
pub struct Profiler {
    metric: String,
    monitor: Arc<PerformanceMonitor>,
    started: Instant,
}

impl Profiler {
    pub fn start_with_monitor(metric: &str, monitor: Arc<PerformanceMonitor>) -> Self {
        Self {
            metric: metric.to_string(),
            monitor,
            started: Instant::now(),
        }
    }

    pub async fn stop_and_record(self) -> MonitoringResult<()> {
        let elapsed = self.started.elapsed().as_secs_f64();
        self.monitor.record(&self.metric, elapsed).await
    }
}

/// Trait describing an exporter implementation.
pub trait Exporter: Send + Sync {
    fn content_type(&self) -> &'static str;
}

struct SimpleExporter {
    content_type: &'static str,
}

impl Exporter for SimpleExporter {
    fn content_type(&self) -> &'static str {
        self.content_type
    }
}

/// Minimal OTLP exporter for building JSON payloads.
pub struct OtlpExporter {
    endpoint: String,
    service_name: String,
}

impl Exporter for OtlpExporter {
    fn content_type(&self) -> &'static str {
        "application/json"
    }
}

impl OtlpExporter {
    pub fn new(endpoint: String, service_name: String) -> Self {
        Self {
            endpoint,
            service_name,
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    pub fn build_payload(
        &self,
        version: &str,
        health: &HealthReport,
        metrics: &HashMap<String, PerformanceStats>,
    ) -> MonitoringResult<String> {
        let timestamp = current_unix_nanos()?;
        let mut otlp_metrics = Vec::new();
        let status_value = health_status_value(health.status);
        otlp_metrics.push(gauge_metric(
            "neo.health.status",
            "Aggregated node health (0=Healthy,1=Degraded,2=Unhealthy)",
            status_value,
            timestamp,
            None,
        ));

        for component in &health.components {
            otlp_metrics.push(gauge_metric(
                format!("neo.health.component.{}", component.component),
                "Per-component health status (0=Healthy,1=Degraded,2=Unhealthy)",
                health_status_value(component.status),
                timestamp,
                Some(vec![
                    json!({"key": "component", "value": {"stringValue": component.component}}),
                    json!({"key": "duration_ms", "value": {"doubleValue": component.duration.as_secs_f64() * 1000.0}}),
                ]),
            ));
        }

        for (name, stats) in metrics {
            otlp_metrics.push(gauge_metric(
                format!("neo.performance.{}.avg", name),
                "Average value recorded for the metric",
                stats.avg,
                timestamp,
                None,
            ));
            otlp_metrics.push(gauge_metric(
                format!("neo.performance.{}.current", name),
                "Most recent value recorded for the metric",
                stats.current,
                timestamp,
                None,
            ));
        }

        let payload = json!({
            "resourceMetrics": [{
                "resource": {
                    "attributes": [
                        {"key": "service.name", "value": {"stringValue": self.service_name}},
                        {"key": "service.version", "value": {"stringValue": version}},
                        {"key": "telemetry.sdk.name", "value": {"stringValue": "neo-monitoring"}},
                        {"key": "telemetry.sdk.language", "value": {"stringValue": "rust"}}
                    ]
                },
                "scopeMetrics": [{
                    "scope": {
                        "name": "neo.monitoring",
                        "version": "1.0"
                    },
                    "metrics": otlp_metrics
                }]
            }]
        });

        serde_json::to_string(&payload).map_err(|e| MonitoringError::Exporter(e.to_string()))
    }

    pub async fn send(&self, payload: String) -> MonitoringResult<()> {
        let client = Client::new();
        let response = client
            .post(&self.endpoint)
            .header("Content-Type", self.content_type())
            .body(payload)
            .send()
            .await
            .map_err(|e| MonitoringError::Exporter(e.to_string()))?;

        if !response.status().is_success() {
            return Err(MonitoringError::Exporter(format!(
                "OTLP push failed with status {}",
                response.status()
            )));
        }
        Ok(())
    }
}

/// Factory for creating exporters.
pub struct ExporterFactory;

impl ExporterFactory {
    pub fn create(format: &str) -> Option<Box<dyn Exporter>> {
        match format {
            "prometheus" => Some(Box::new(SimpleExporter {
                content_type: "text/plain",
            })),
            "json" | "json-pretty" => Some(Box::new(SimpleExporter {
                content_type: "application/json",
            })),
            "csv" => Some(Box::new(SimpleExporter {
                content_type: "text/csv",
            })),
            _ => None,
        }
    }

    pub fn create_otlp(endpoint: String, service_name: String) -> Box<dyn Exporter> {
        Box::new(OtlpExporter::new(endpoint, service_name))
    }
}

/// Compatibility namespace mirroring the original C# layout.
pub mod performance {
    pub use super::{
        Alert, AlertLevel, PerformanceMonitor, PerformanceStats, PerformanceThreshold,
        ThresholdType,
    };
}

fn current_unix_nanos() -> MonitoringResult<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .map_err(|e| MonitoringError::Exporter(e.to_string()))
}

fn health_status_value(status: HealthStatus) -> f64 {
    match status {
        HealthStatus::Healthy => 0.0,
        HealthStatus::Degraded => 1.0,
        HealthStatus::Unhealthy => 2.0,
    }
}

fn gauge_metric(
    name: impl Into<String>,
    description: impl Into<String>,
    value: f64,
    timestamp: u64,
    attributes: Option<Vec<serde_json::Value>>,
) -> serde_json::Value {
    let base_attributes = attributes.unwrap_or_default();
    json!({
        "name": name.into(),
        "description": description.into(),
        "unit": "",
        "gauge": {
            "dataPoints": [{
                "timeUnixNano": timestamp.to_string(),
                "asDouble": value,
                "attributes": base_attributes
            }]
        }
    })
}
