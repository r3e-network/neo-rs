use std::time::Duration;

/// Outcome and timing for a single RPC call.
#[derive(Debug, Clone)]
pub struct RpcRequestOutcome {
    /// RPC method name.
    pub method: String,
    /// Total elapsed time for the request.
    pub elapsed: Duration,
    /// Whether the request completed successfully.
    pub success: bool,
    /// Timeout configured for the request.
    pub timeout: Duration,
    /// JSON-RPC error code, when the call returned an RPC error.
    pub error_code: Option<i32>,
}

/// Observer for RPC request outcomes.
pub trait RpcObserver: Clone + Send + Sync + 'static {
    /// Called after an RPC request completes.
    fn observe(&self, outcome: &RpcRequestOutcome);
}

/// Default observer that emits tracing debug events for completed requests.
#[derive(Clone, Copy, Debug, Default)]
pub struct TracingRpcObserver;

impl RpcObserver for TracingRpcObserver {
    fn observe(&self, outcome: &RpcRequestOutcome) {
        tracing::debug!(
            method = %outcome.method,
            elapsed_ms = outcome.elapsed.as_millis() as u64,
            success = outcome.success,
            timeout_ms = outcome.timeout.as_millis() as u64,
            error_code = outcome.error_code,
            "rpc request finished"
        );
    }
}

impl<F> RpcObserver for F
where
    F: Fn(&RpcRequestOutcome) + Clone + Send + Sync + 'static,
{
    fn observe(&self, outcome: &RpcRequestOutcome) {
        self(outcome);
    }
}

/// Observability hooks for RPC client requests.
#[derive(Clone, Default)]
pub struct RpcClientHooks<O = TracingRpcObserver> {
    observer: O,
}

impl RpcClientHooks<TracingRpcObserver> {
    /// Returns a hook collection using the default tracing observer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl<O> RpcClientHooks<O>
where
    O: RpcObserver,
{
    /// Registers an observer called after each RPC request completes.
    pub fn with_observer<T>(self, observer: T) -> RpcClientHooks<T>
    where
        T: RpcObserver,
    {
        let _ = self;
        RpcClientHooks { observer }
    }

    pub(crate) fn notify(&self, outcome: RpcRequestOutcome) {
        self.observer.observe(&outcome);
    }
}
