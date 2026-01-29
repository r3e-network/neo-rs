//! Idle actor - matches C# Neo.IO.Actors.Idle exactly

use std::sync::OnceLock;

/// Idle type matching C# Neo.IO.Actors.Idle
pub struct ActorIdle;

/// Backwards-compatible alias so modules can continue importing `Idle`.
pub type Idle = ActorIdle;

impl ActorIdle {
    /// Gets the singleton instance (matches C# Instance property)
    pub fn instance() -> &'static Self {
        static INSTANCE: OnceLock<ActorIdle> = OnceLock::new();
        INSTANCE.get_or_init(|| Self)
    }
}
