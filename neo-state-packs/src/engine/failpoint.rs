//! Process-crash failpoints for deterministic durability tests.
//!
//! Default builds compile these calls to no-ops. Unit tests and explicit
//! `fault-injection` builds may select one boundary through an environment
//! variable; reaching it terminates without unwinding so RAII cleanup cannot
//! erase the crash artifact being tested.

#[cfg(any(test, feature = "fault-injection"))]
pub(crate) const ENVIRONMENT_VARIABLE: &str = "NEO_STATE_PACKS_FAILPOINT";
#[cfg(any(test, feature = "fault-injection"))]
pub(crate) const EXIT_CODE: i32 = 86;

#[inline]
pub(crate) fn crash(name: &str) {
    #[cfg(any(test, feature = "fault-injection"))]
    if std::env::var(ENVIRONMENT_VARIABLE).as_deref() == Ok(name) {
        std::process::exit(EXIT_CODE);
    }

    #[cfg(not(any(test, feature = "fault-injection")))]
    let _ = name;
}
