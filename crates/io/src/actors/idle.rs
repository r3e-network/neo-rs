//! Idle actor - matches C# Neo.IO.Actors.Idle exactly

use std::sync::OnceLock;

/// Idle type matching C# Neo.IO.Actors.Idle
pub struct Idle;

impl Idle {
    /// Gets the singleton instance (matches C# Instance property)
    pub fn instance() -> &'static Idle {
        static INSTANCE: OnceLock<Idle> = OnceLock::new();
        INSTANCE.get_or_init(|| Idle)
    }
}
