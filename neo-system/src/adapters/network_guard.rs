//! Constructor-injected network guard (decision L.4).
//!
//! The legacy plugin collaborators (`application_logs`, `tokens_tracker`)
//! protected themselves against being driven by the wrong network with a
//! *runtime* `Any` downcast on **every** block:
//!
//! ```rust,ignore
//! // BEFORE (per-invocation, neo-core coupled):
//! let Some(system) = system.downcast_ref::<NeoSystem>() else { return; };
//! if system.settings().network != self.settings.network {
//!     return; // silently skip the block
//! }
//! ```
//!
//! That pattern has three problems L.4 removes:
//!
//! 1. it reaches through `&dyn Any` into a concrete `NeoSystem`, coupling the
//!    collaborator to `neo-core`;
//! 2. it re-checks on every block even though the network can never change after
//!    startup; and
//! 3. a mismatch degrades into a silent skip instead of a refused start.
//!
//! [`NetworkGuard`] replaces it with **constructor injection**: the expected
//! network magic is supplied once, compared once at startup, and a mismatch
//! refuses to start rather than being swallowed at runtime. No `Any` downcast is
//! retained anywhere in this module.
//!
//! # Type discipline
//!
//! Only `std` is named here. The identifier is the plain `u32` network magic
//! carried by `ProtocolSettings::network` and by the `MAINNET_MAGIC` /
//! `TESTNET_MAGIC` constants in `neo_primitives::constants`; the wiring lane
//! (task 89) reads that value from config and passes it in.

use std::fmt;

/// Failure returned when a component is started against the wrong network.
///
/// Carries both magics so the operator sees exactly what was expected versus what
/// the host actually reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkMismatch {
    /// The magic injected at construction time.
    pub expected: u32,
    /// The magic reported by the host at startup.
    pub actual: u32,
}

impl NetworkMismatch {
    /// Builds a mismatch from the expected and observed magics.
    pub const fn new(expected: u32, actual: u32) -> Self {
        Self { expected, actual }
    }
}

impl fmt::Display for NetworkMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "network guard rejected startup: expected network 0x{:08x}, but host is 0x{:08x}",
            self.expected, self.actual
        )
    }
}

impl std::error::Error for NetworkMismatch {}

/// One-time startup guard comparing an injected network magic against the host's.
///
/// Construct it with the network the component was configured for, then call
/// [`verify`](NetworkGuard::verify) exactly once during startup. A mismatch is an
/// error that must abort startup; a match means the component may proceed and
/// never needs to re-check the network again — removing the per-block `Any`
/// downcast entirely.
///
/// # Example
///
/// ```rust,ignore
/// // `configured_magic` comes from the component's own settings; `host_magic`
/// // from the running system's protocol settings. Both are plain u32.
/// let guard = NetworkGuard::new(configured_magic);
/// guard.verify(host_magic)?; // refuse to start on mismatch
/// // ... startup continues; no per-block downcast required.
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkGuard {
    expected_network: u32,
}

impl NetworkGuard {
    /// Injects the expected network magic at construction time.
    pub const fn new(expected_network: u32) -> Self {
        Self { expected_network }
    }

    /// Returns the injected expected network magic.
    pub const fn expected_network(&self) -> u32 {
        self.expected_network
    }

    /// Performs the one-time startup comparison.
    ///
    /// Returns `Ok(())` when `actual_network` matches the injected expectation,
    /// or a [`NetworkMismatch`] error that must abort startup otherwise. This is
    /// the single comparison that replaces the legacy per-block `Any` downcast.
    pub const fn verify(&self, actual_network: u32) -> Result<(), NetworkMismatch> {
        if self.expected_network == actual_network {
            Ok(())
        } else {
            Err(NetworkMismatch::new(self.expected_network, actual_network))
        }
    }

    /// Returns `true` when `actual_network` matches the injected expectation.
    ///
    /// A non-fatal predicate for callers that want to branch instead of abort;
    /// startup paths should prefer [`verify`](NetworkGuard::verify) so a mismatch
    /// cannot be ignored.
    pub const fn matches(&self, actual_network: u32) -> bool {
        self.expected_network == actual_network
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::constants::{MAINNET_MAGIC, TESTNET_MAGIC};

    #[test]
    fn matching_network_passes_the_guard() {
        let guard = NetworkGuard::new(MAINNET_MAGIC);
        assert!(guard.verify(MAINNET_MAGIC).is_ok());
        assert!(guard.matches(MAINNET_MAGIC));
        assert_eq!(guard.expected_network(), MAINNET_MAGIC);
    }

    #[test]
    fn mismatched_network_refuses_startup() {
        let guard = NetworkGuard::new(MAINNET_MAGIC);
        let err = guard.verify(TESTNET_MAGIC).unwrap_err();

        assert_eq!(err.expected, MAINNET_MAGIC);
        assert_eq!(err.actual, TESTNET_MAGIC);
        assert!(!guard.matches(TESTNET_MAGIC));
        assert!(err.to_string().contains("rejected startup"));
    }

    #[test]
    fn mismatch_display_shows_both_magics_in_hex() {
        let err = NetworkMismatch::new(MAINNET_MAGIC, TESTNET_MAGIC);
        let text = err.to_string();
        assert!(text.contains("0x334f454e")); // MAINNET_MAGIC
        assert!(text.contains("0x3554334e")); // TESTNET_MAGIC
    }

    #[test]
    fn mismatch_is_a_std_error() {
        fn assert_error<E: std::error::Error>() {}
        assert_error::<NetworkMismatch>();
    }
}
