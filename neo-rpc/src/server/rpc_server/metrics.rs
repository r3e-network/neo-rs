//! Prometheus counters for RPC request and error totals.

use std::sync::LazyLock;

use prometheus::Counter;
use tracing::warn;

/// Total number of RPC requests dispatched by this process.
pub static RPC_REQ_TOTAL: LazyLock<Counter> = LazyLock::new(|| {
    let counter =
        Counter::new("neo_rpc_requests_total", "Total RPC requests").unwrap_or_else(|_| {
            Counter::new("neo_rpc_requests_total_invalid", "Invalid")
                .expect("fallback counter creation should never fail")
        });
    if let Err(err) = prometheus::register(Box::new(counter.clone())) {
        warn!("Failed to register neo_rpc_requests_total: {}", err);
    }
    counter
});

/// Total number of RPC requests that returned an RPC error.
pub static RPC_ERR_TOTAL: LazyLock<Counter> = LazyLock::new(|| {
    let counter = Counter::new("neo_rpc_errors_total", "Total RPC errors").unwrap_or_else(|_| {
        Counter::new("neo_rpc_errors_total_invalid", "Invalid")
            .expect("fallback counter creation should never fail")
    });
    if let Err(err) = prometheus::register(Box::new(counter.clone())) {
        warn!("Failed to register neo_rpc_errors_total: {}", err);
    }
    counter
});
