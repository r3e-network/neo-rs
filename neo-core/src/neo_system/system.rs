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
