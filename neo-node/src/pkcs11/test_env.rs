//! Serialized mutation of the `NEO_HSM_PIN` environment variable.
//!
//! The environment is process-global, so every test that sets or removes the
//! PIN must hold [`pin_env_lock`] for the duration of its assertions to stay
//! deterministic under the default parallel test runner.

use std::sync::{Mutex, MutexGuard, OnceLock};

/// Acquires the cross-module lock guarding `NEO_HSM_PIN` mutations.
pub(crate) fn pin_env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

/// Sets or removes the PIN environment variable. The caller must hold
/// [`pin_env_lock`].
pub(crate) fn set_pin(pin: Option<&str>) {
    match pin {
        Some(value) => unsafe { std::env::set_var(super::NEO_HSM_PIN_ENV, value) },
        None => unsafe { std::env::remove_var(super::NEO_HSM_PIN_ENV) },
    }
}
