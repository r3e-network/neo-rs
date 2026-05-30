//! Broadcast intents executed by the local node actor.

/// Captures different broadcast intents executed by the local node actor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BroadcastEvent {
    /// Relay broadcast to all connected peers.
    Relay(Vec<u8>),
    /// Direct broadcast to specific peers.
    Direct(Vec<u8>),
}
