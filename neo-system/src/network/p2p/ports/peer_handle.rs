//! Opaque peer-session identity for the typed P2P ports.
//!
//! The C# / `neo-core` networking stack identifies a connected peer by its
//! actor reference (`ActorRef`). Naming that runtime type inside `neo-p2p`
//! would create a reverse dependency on the actor/system layer, so every port
//! leaf instead forwards this opaque handle. Adapters that own the actor
//! runtime mint a handle when a session registers and translate it back to the
//! concrete peer when delivering a command; the P2P layer only compares,
//! hashes, copies, and forwards it.

/// Opaque identifier for a connected peer session.
///
/// The inner value is private: there is deliberately no `Default`, no public
/// field, and no way to fabricate a "zero" peer, so a port command can never
/// silently target an unregistered session. Handles are `Copy` so they stay
/// cheap to pass across every port boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PeerHandle(u64);

impl PeerHandle {
    /// Wraps a raw session identifier.
    ///
    /// Only the adapter that owns the peer/session registry should mint
    /// handles; the identifier policy (e.g. an actor-path hash or a slot index)
    /// is an adapter concern and is intentionally not exposed here.
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the raw identifier for adapter-side correlation.
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for PeerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "peer#{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_equality_and_raw_access_are_stable() {
        let a = PeerHandle::new(7);
        let b = PeerHandle::new(7);
        let c = PeerHandle::new(8);

        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.as_u64(), 7);
    }

    #[test]
    fn handle_is_copy_and_displayable() {
        let a = PeerHandle::new(3);
        // `Copy` keeps a handle cheap to forward across every port call.
        let copied = a;
        assert_eq!(copied, a);
        assert_eq!(a.to_string(), "peer#3");
    }

    #[test]
    fn distinct_handles_hash_distinctly() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        assert!(set.insert(PeerHandle::new(1)));
        assert!(!set.insert(PeerHandle::new(1)));
        assert!(set.insert(PeerHandle::new(2)));
        assert_eq!(set.len(), 2);
    }
}
