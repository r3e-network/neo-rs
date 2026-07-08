//! Prometheus counters for RPC request and error totals.

use std::sync::OnceLock;

use prometheus::Counter;
use tracing::warn;

/// Lazily registered RPC counter.
///
/// Metric construction failures should not take down the RPC server. Static
/// metric names are expected to be valid; if construction ever fails, the
/// counter becomes a logged no-op instead of a panic path.
pub struct RpcCounter {
    name: &'static str,
    help: &'static str,
    counter: OnceLock<Option<Counter>>,
}

impl RpcCounter {
    const fn new(name: &'static str, help: &'static str) -> Self {
        Self {
            name,
            help,
            counter: OnceLock::new(),
        }
    }

    /// Increments the counter by one when the Prometheus metric is available.
    pub fn inc(&self) {
        if let Some(counter) = self.counter() {
            counter.inc();
        }
    }

    /// Returns the current counter value, or zero when metric construction failed.
    #[cfg(test)]
    pub fn get(&self) -> f64 {
        self.counter().map(Counter::get).unwrap_or(0.0)
    }

    fn counter(&self) -> Option<&Counter> {
        self.counter.get_or_init(|| self.build()).as_ref()
    }

    fn build(&self) -> Option<Counter> {
        let counter = match Counter::new(self.name, self.help) {
            Ok(counter) => counter,
            Err(err) => {
                warn!("Failed to create {}: {}", self.name, err);
                return None;
            }
        };
        if let Err(err) = prometheus::register(Box::new(counter.clone())) {
            warn!("Failed to register {}: {}", self.name, err);
        }
        Some(counter)
    }
}

/// Total number of RPC requests dispatched by this process.
pub static RPC_REQ_TOTAL: RpcCounter =
    RpcCounter::new("neo_rpc_requests_total", "Total RPC requests");

/// Total number of RPC requests that returned an RPC error.
pub static RPC_ERR_TOTAL: RpcCounter = RpcCounter::new("neo_rpc_errors_total", "Total RPC errors");

#[cfg(test)]
#[path = "../../tests/server/rpc_server/metrics.rs"]
mod tests;
