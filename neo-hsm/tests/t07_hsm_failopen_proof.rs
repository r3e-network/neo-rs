//! T07 security audit — empirical proofs for HSM findings.
//!
//! ADDITIVE ONLY (new test file); no `src/` file is modified.

use neo_hsm::{HsmSigner, SimulationSigner};

/// T07-H1: `SimulationSigner::unlock` is fail-open when no PIN is configured.
///
/// `unlock` only compares the supplied PIN when `self.pin` is `Some(_)`. When no
/// PIN has been set (the default construction path, and the state of
/// `SimulationSigner::with_test_key()` used by `neo-node --hsm-device simulation`)
/// ANY string unlocks the signer and flips `is_ready` to true.
///
/// `neo-node`'s `initialize_hsm` relies on exactly this: after the
/// `requires_pin` gate (false for `HsmDeviceInfo::simulation()`), it calls
/// `signer.unlock("")` whenever `!signer.is_ready()`.
#[tokio::test]
async fn unlock_succeeds_with_arbitrary_pin_when_no_pin_configured() {
    let signer = SimulationSigner::with_test_key().expect("signer");
    assert!(signer.is_ready());

    // Lock it, then "unlock" with a value that was never configured.
    signer.lock();
    assert!(signer.is_locked());

    signer
        .unlock("not-a-real-pin")
        .await
        .expect("ARBITRARY PIN UNLOCKED THE HSM — fail-open");

    assert!(!signer.is_locked());
}

/// T07-H2: `sign` only gates on `is_locked()`, never on `is_ready()`.
///
/// A freshly constructed `SimulationSigner` is unlocked by default
/// (`is_locked: RwLock::new(false)`), so it will sign before any `unlock()`
/// call and before `is_ready()` is true.
#[tokio::test]
async fn sign_works_before_unlock_and_without_ready() {
    let signer = SimulationSigner::new();
    signer.generate_key("k", None).expect("generate");

    assert!(!signer.is_ready(), "signer is not ready yet");
    assert!(!signer.is_locked(), "but it is already unlocked");

    let sig = signer
        .sign("k", b"payload")
        .await
        .expect("SIGNED WITHOUT UNLOCK — sign() ignores is_ready()");
    assert_eq!(sig.len(), 64);
}

/// T07-H3: no PIN attempt throttling exists in the simulation backend.
///
/// `HsmConfig::max_pin_attempts` (default 3) is never consulted by any code
/// path in `neo-hsm`; `HsmError::PinLocked` is only produced by the
/// feature-gated `ledger`/`pkcs11` backends.
#[tokio::test]
async fn pin_guessing_is_not_throttled() {
    let signer = SimulationSigner::new();
    signer.generate_key("k", None).expect("generate");
    signer.set_pin("1234");

    for attempt in 0..100 {
        let err = signer
            .unlock(&format!("wrong-{attempt}"))
            .await
            .expect_err("wrong pin must fail");
        assert!(matches!(err, neo_hsm::HsmError::InvalidPin));
    }

    // After 100 wrong attempts the correct PIN still works: no lockout.
    signer.unlock("1234").await.expect("no lockout after 100 tries");
}
