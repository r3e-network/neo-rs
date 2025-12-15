//! Metrics recorder for collecting and storing telemetry data.

use parking_lot::RwLock;
use std::collections::HashMap;

use super::metrics::{Counter, Gauge, Histogram};

/// Central metrics recorder that stores all metrics.
#[derive(Default)]
pub struct MetricsRecorder {
    counters: RwLock<HashMap<String, Counter>>,
    gauges: RwLock<HashMap<String, Gauge>>,
    histograms: RwLock<HashMap<String, Histogram>>,
}

impl MetricsRecorder {
    /// Creates a new metrics recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a gauge value.
    pub fn record_gauge(&self, name: &str, value: f64) {
        let gauges = self.gauges.read();
        if let Some(gauge) = gauges.get(name) {
            gauge.set(value);
            return;
        }
        drop(gauges);

        let mut gauges = self.gauges.write();
        let gauge = gauges.entry(name.to_string()).or_default();
        gauge.set(value);
    }

    /// Increments a counter.
    pub fn increment_counter(&self, name: &str, amount: u64) {
        let counters = self.counters.read();
        if let Some(counter) = counters.get(name) {
            counter.increment_by(amount);
            return;
        }
        drop(counters);

        let mut counters = self.counters.write();
        let counter = counters.entry(name.to_string()).or_default();
        counter.increment_by(amount);
    }

    /// Records a histogram observation.
    pub fn record_histogram(&self, name: &str, value: f64) {
        let histograms = self.histograms.read();
        if let Some(histogram) = histograms.get(name) {
            histogram.observe(value);
            return;
        }
        drop(histograms);

        let mut histograms = self.histograms.write();
        let histogram = histograms.entry(name.to_string()).or_default();
        histogram.observe(value);
    }

    /// Returns a snapshot of all current metrics.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let counters = self.counters.read();
        let gauges = self.gauges.read();
        let histograms = self.histograms.read();

        MetricsSnapshot {
            counters: counters.iter().map(|(k, v)| (k.clone(), v.get())).collect(),
            gauges: gauges.iter().map(|(k, v)| (k.clone(), v.get())).collect(),
            histograms: histograms
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        HistogramSnapshot {
                            count: v.count(),
                            sum: v.sum(),
                            min: v.min(),
                            max: v.max(),
                            avg: v.avg(),
                        },
                    )
                })
                .collect(),
        }
    }

    /// Returns the current value of a gauge, if it exists.
    pub fn get_gauge(&self, name: &str) -> Option<f64> {
        self.gauges.read().get(name).map(|g| g.get())
    }

    /// Returns the current value of a counter, if it exists.
    pub fn get_counter(&self, name: &str) -> Option<u64> {
        self.counters.read().get(name).map(|c| c.get())
    }

    /// Clears all metrics.
    pub fn clear(&self) {
        self.counters.write().clear();
        self.gauges.write().clear();
        self.histograms.write().clear();
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
