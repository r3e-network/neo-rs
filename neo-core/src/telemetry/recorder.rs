//! Metrics recorder for collecting and storing telemetry data.

use dashmap::DashMap;
use std::collections::HashMap;

use super::metrics::{Counter, Gauge, Histogram};

/// Central metrics recorder that stores all metrics.
#[derive(Default)]
pub struct MetricsRecorder {
    counters: DashMap<String, Counter>,
    gauges: DashMap<String, Gauge>,
    histograms: DashMap<String, Histogram>,
}

impl MetricsRecorder {
    /// Creates a new metrics recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a gauge value.
    pub fn record_gauge(&self, name: &str, value: f64) {
        if let Some(gauge) = self.gauges.get(name) {
            gauge.set(value);
            return;
        }

        self.gauges.entry(name.to_string()).or_default().set(value);
    }

    /// Increments a counter.
    pub fn increment_counter(&self, name: &str, amount: u64) {
        if let Some(counter) = self.counters.get(name) {
            counter.increment_by(amount);
            return;
        }

        self.counters
            .entry(name.to_string())
            .or_default()
            .increment_by(amount);
    }

    /// Records a histogram observation.
    pub fn record_histogram(&self, name: &str, value: f64) {
        if let Some(histogram) = self.histograms.get(name) {
            histogram.observe(value);
            return;
        }

        self.histograms
            .entry(name.to_string())
            .or_default()
            .observe(value);
    }

    /// Returns a snapshot of all current metrics.
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            counters: self
                .counters
                .iter()
                .map(|entry| (entry.key().clone(), entry.value().get()))
                .collect(),
            gauges: self
                .gauges
                .iter()
                .map(|entry| (entry.key().clone(), entry.value().get()))
                .collect(),
            histograms: self
                .histograms
                .iter()
                .map(|entry| {
                    let histogram = entry.value();
                    (
                        entry.key().clone(),
                        HistogramSnapshot {
                            count: histogram.count(),
                            sum: histogram.sum(),
                            min: histogram.min(),
                            max: histogram.max(),
                            avg: histogram.avg(),
                        },
                    )
                })
                .collect(),
        }
    }

    /// Returns the current value of a gauge, if it exists.
    pub fn get_gauge(&self, name: &str) -> Option<f64> {
        self.gauges.get(name).map(|g| g.get())
    }

    /// Returns the current value of a counter, if it exists.
    pub fn get_counter(&self, name: &str) -> Option<u64> {
        self.counters.get(name).map(|c| c.get())
    }

    /// Clears all metrics.
    pub fn clear(&self) {
        self.counters.clear();
        self.gauges.clear();
        self.histograms.clear();
    }
}

/// Snapshot of histogram statistics.
#[derive(Debug, Clone)]
pub struct HistogramSnapshot {
    pub count: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub avg: f64,
}

/// Snapshot of all metrics at a point in time.
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub counters: HashMap<String, u64>,
    pub gauges: HashMap<String, f64>,
    pub histograms: HashMap<String, HistogramSnapshot>,
}

impl MetricsSnapshot {
    /// Exports metrics in Prometheus text format.
    pub fn to_prometheus_text(&self) -> String {
        let mut output = String::new();

        for (name, value) in &self.counters {
            output.push_str(&format!("# TYPE {} counter\n{} {}\n", name, name, value));
        }

        for (name, value) in &self.gauges {
            output.push_str(&format!("# TYPE {} gauge\n{} {}\n", name, name, value));
        }

        for (name, hist) in &self.histograms {
            output.push_str(&format!("# TYPE {} histogram\n", name));
            output.push_str(&format!("{}_count {}\n", name, hist.count));
            output.push_str(&format!("{}_sum {}\n", name, hist.sum));
        }

        output
    }

    /// Exports metrics as JSON.
    pub fn to_json(&self) -> String {
        serde_json::json!({
            "counters": self.counters,
            "gauges": self.gauges,
            "histograms": self.histograms.iter().map(|(k, v)| {
                (k.clone(), serde_json::json!({
                    "count": v.count,
                    "sum": v.sum,
                    "min": v.min,
                    "max": v.max,
                    "avg": v.avg
                }))
            }).collect::<HashMap<_, _>>()
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_stores_gauges() {
        let recorder = MetricsRecorder::new();
        recorder.record_gauge("test", 42.0);
        assert_eq!(recorder.get_gauge("test"), Some(42.0));
    }

    #[test]
    fn recorder_stores_counters() {
        let recorder = MetricsRecorder::new();
        recorder.increment_counter("test", 1);
        recorder.increment_counter("test", 2);
        assert_eq!(recorder.get_counter("test"), Some(3));
    }

    #[test]
    fn snapshot_exports_prometheus_format() {
        let recorder = MetricsRecorder::new();
        recorder.record_gauge("block_height", 100.0);
        recorder.increment_counter("transactions", 50);

        let snapshot = recorder.snapshot();
        let prometheus = snapshot.to_prometheus_text();

        assert!(prometheus.contains("block_height 100"));
        assert!(prometheus.contains("transactions 50"));
    }

    #[test]
    fn snapshot_exports_json() {
        let recorder = MetricsRecorder::new();
        recorder.record_gauge("test_gauge", 42.0);

        let snapshot = recorder.snapshot();
        let json = snapshot.to_json();

        assert!(json.contains("test_gauge"));
        assert!(json.contains("42"));
    }
}
