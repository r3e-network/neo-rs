//! Core NeoSystem types and readiness status.
//!
//! This module contains the fundamental types for the Neo node system:
//! - `ReadinessStatus` - Snapshot of node liveness/sync state
//! - Service name constants

/// Named service keys registered into the NeoSystem service registry.
pub const STATE_STORE_SERVICE: &str = "StateStore";

/// Snapshot of basic liveness/sync state for readiness checks.
///
/// This struct provides a point-in-time view of the node's synchronization
/// status and service availability, used by health checks and monitoring.
#[derive(Debug, Clone, Copy)]
pub struct ReadinessStatus {
    /// Current block height in the ledger.
    pub block_height: u32,
    /// Current header height (may be ahead of block height during sync).
    pub header_height: u32,
    /// Number of headers ahead of blocks (sync lag indicator).
    pub header_lag: u32,
    /// Overall health status (sync within acceptable lag).
    pub healthy: bool,
    /// Whether the RPC service is available.
    pub rpc_ready: bool,
    /// Whether storage is ready and accessible.
    pub storage_ready: bool,
}

impl ReadinessStatus {
    /// Creates a new readiness status with default unhealthy state.
    pub fn unhealthy() -> Self {
        Self {
            block_height: 0,
            header_height: 0,
            header_lag: u32::MAX,
            healthy: false,
            rpc_ready: false,
            storage_ready: false,
        }
    }

    /// Annotates the readiness snapshot with service readiness and updates the overall health flag.
    pub fn with_services(mut self, rpc_ready: bool, storage_ready: bool) -> Self {
        self.rpc_ready = rpc_ready;
        self.storage_ready = storage_ready;
        self.healthy = self.healthy && rpc_ready && storage_ready;
        self
    }
}

// Readiness helpers for the main `NeoSystem`.
impl super::core::NeoSystem {
    /// Basic readiness snapshot (ledger sync only). Consumers can layer on service checks (RPC, storage, etc.).
    pub fn readiness(&self, max_header_lag: Option<u32>) -> ReadinessStatus {
        let (block_height, header_height) = if let Ok(Some(ledger)) = self.ledger_typed() {
            (ledger.current_height(), ledger.current_header_height())
        } else {
            let ledger = self.ledger_context();
            (ledger.current_height(), ledger.highest_header_index())
        };
        let header_lag = header_height.saturating_sub(block_height);
        let healthy = max_header_lag
            .map(|threshold| header_lag <= threshold)
            .unwrap_or(true);

        ReadinessStatus {
            block_height,
            header_height,
            header_lag,
            healthy,
            rpc_ready: true,
            storage_ready: true,
        }
    }

    /// Readiness snapshot annotated with optional service and storage readiness flags.
    pub fn readiness_with_services(
        &self,
        max_header_lag: Option<u32>,
        rpc_service_name: Option<&str>,
        storage_ready: Option<bool>,
    ) -> ReadinessStatus {
        let status = self.readiness(max_header_lag);
        let rpc_ready = rpc_service_name
            .map(|name| self.has_named_service(name))
            .unwrap_or(true);
        let storage_ready = storage_ready.unwrap_or(true);
        status.with_services(rpc_ready, storage_ready)
    }

    /// Convenience wrapper that uses the configured RPC service name for this network.
    pub fn readiness_with_defaults(
        &self,
        max_header_lag: Option<u32>,
        storage_ready: Option<bool>,
    ) -> ReadinessStatus {
        self.readiness_with_services(
            max_header_lag,
            Some(&self.rpc_service_name()),
            storage_ready,
        )
    }

    /// Returns `true` when the node is considered ready (sync within the given lag).
    pub fn is_ready(&self, max_header_lag: Option<u32>) -> bool {
        self.readiness(max_header_lag).healthy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_status_unhealthy_default() {
        let status = ReadinessStatus::unhealthy();
        assert!(!status.healthy);
        assert_eq!(status.header_lag, u32::MAX);
    }

    #[test]
    fn readiness_status_with_services_updates_health() {
        let status = ReadinessStatus {
            block_height: 100,
            header_height: 100,
            header_lag: 0,
            healthy: true,
            rpc_ready: false,
            storage_ready: false,
        };

        let updated = status.with_services(true, true);
        assert!(updated.healthy);
        assert!(updated.rpc_ready);
        assert!(updated.storage_ready);

        let status2 = ReadinessStatus {
            block_height: 100,
            header_height: 100,
            header_lag: 0,
            healthy: true,
            rpc_ready: false,
            storage_ready: false,
        };

        let updated2 = status2.with_services(false, true);
        assert!(!updated2.healthy);
    }
}
