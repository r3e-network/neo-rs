
use std::time::{SystemTime, UNIX_EPOCH};
use once_cell::sync::Lazy;
use std::sync::RwLock;

/// The time provider for the NEO system.
pub struct TimeProvider;

impl TimeProvider {
    /// Gets the current time expressed as the Coordinated Universal Time (UTC).
    pub fn utc_now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs()
    }
}

static DEFAULT: Lazy<TimeProvider> = Lazy::new(|| TimeProvider);

/// The currently used TimeProvider instance.
static CURRENT: Lazy<RwLock<&'static TimeProvider>> = Lazy::new(|| RwLock::new(&*DEFAULT));

/// Get the current TimeProvider instance.
pub fn current() -> &'static TimeProvider {
    *CURRENT.read().unwrap()
}

/// Set the current TimeProvider instance. This function is not public and should only be used internally.
pub(crate) fn set_current(provider: &'static TimeProvider) {
    *CURRENT.write().unwrap() = provider;
}

/// Reset the TimeProvider to the default instance. This function is not public and should only be used internally.
pub(crate) fn reset_to_default() {
    set_current(&*DEFAULT);
}
