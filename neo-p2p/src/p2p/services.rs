//! Service trait definitions for the P2P subsystem.
//!
//! Distributed from `neo-core::services::traits`.

/// Peer manager operations exposed to observability layers.
///
/// Provides peer connection statistics. This is a minimal observability
/// subset of the full [`PeerManager`](crate::traits::PeerManager) trait.
pub trait PeerManagerService: Send + Sync {
    /// Current connected peer count.
    fn peer_count(&self) -> usize;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockPeerManager {
        count: usize,
    }

    impl PeerManagerService for MockPeerManager {
        fn peer_count(&self) -> usize {
            self.count
        }
    }

    #[test]
    fn mock_peer_manager_service_works() {
        let pm = MockPeerManager { count: 7 };
        assert_eq!(pm.peer_count(), 7);
    }

    #[test]
    fn trait_is_object_safe() {
        fn _accepts_peer_manager(_: &dyn PeerManagerService) {}
    }
}
