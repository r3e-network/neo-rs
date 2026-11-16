// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_client/hooks.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use std::sync::Arc;
use std::time::Duration;

/// Outcome and timing for a single RPC call.
#[derive(Debug, Clone)]
pub struct RpcRequestOutcome {
    pub method: String,
    pub elapsed: Duration,
    pub success: bool,
    pub timeout: Duration,
    pub error_code: Option<i32>,
}

/// Hooks that can be used to observe RPC requests for logging/metrics.
type RpcObserverFn = dyn Fn(&RpcRequestOutcome) + Send + Sync;

#[derive(Clone, Default)]
pub struct RpcClientHooks {
    observer: Option<Arc<RpcObserverFn>>,
}

impl RpcClientHooks {
    /// Returns a hook collection without observers (falls back to tracing debug logs).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an observer called after each RPC request completes.
    pub fn with_observer<F>(mut self, observer: F) -> Self
    where
        F: Fn(&RpcRequestOutcome) + Send + Sync + 'static,
    {
        self.observer = Some(Arc::new(observer));
        self
    }

    pub(crate) fn notify(&self, outcome: RpcRequestOutcome) {
        if let Some(observer) = &self.observer {
            observer(&outcome);
        } else {
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
}
