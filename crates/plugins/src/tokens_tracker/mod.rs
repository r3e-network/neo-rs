//! Tokens tracker plugin wiring (stub implementation).
//!
//! The full TokensTracker port from the C# codebase is not yet available in
//! this repository. To keep the workspace compiling when the `tokens-tracker`
//! feature is enabled, we expose a minimal no-op plugin that satisfies the
//! `Plugin` interface. Once the tracker logic is ported, this module can be
//! replaced with the real implementation.

mod stub;

pub use stub::TokensTrackerPlugin;
