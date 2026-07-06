//! Block download ownership mode for per-peer sessions.
//!
//! The network service can either let each peer keep the historical
//! C#-style `GetBlockByIndex` window full, or suppress that legacy loop so a
//! higher-level coordinator owns cross-peer range assignment.

/// Selects which component owns outbound block-sync range requests.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BlockSyncMode {
    /// Each ready peer independently requests blocks while it is ahead.
    ///
    /// This preserves the original compatibility path and remains the default
    /// for tests and embedders that do not compose a coordinator.
    #[default]
    LegacyPerPeer,
    /// Per-peer sessions only serve explicit fetch commands.
    ///
    /// Use this when `BlockDownloadCoordinator` owns range scheduling. Keeping
    /// both paths active would duplicate requests and race imports.
    ExternalCoordinator,
}

impl BlockSyncMode {
    /// Returns whether per-peer sessions should issue automatic sync requests.
    #[must_use]
    pub const fn uses_legacy_per_peer_requests(self) -> bool {
        matches!(self, Self::LegacyPerPeer)
    }
}
