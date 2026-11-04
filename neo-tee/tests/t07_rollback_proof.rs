//! T07/T43 security audit — empirical proofs for TEE sealed-key rollback
//! protection.
//!
//! T07-T4/T4b originally documented the *insecure* self-referential replay
//! floor (`SealedKey::unseal` passed `min_counter = Some(sealed_data.counter)`,
//! which can never reject) as observed behaviour: after a rollback of the
//! sealed-data directory (counter file deleted/restored, counter back at 0)
//! an old sealed blob still unsealed successfully.
//!
//! T43 replaced the self-reference with an EXTERNAL rollback floor witness
//! (`.rollback_floor`, see `neo_tee::enclave::rollback_floor`). The tests
//! below now assert the SECURE behaviour:
//!
//! * a sealed blob whose counter is ahead of the enclave's rolled-back
//!   monotonic counter is REJECTED (fail-closed),
//! * the floor is external: rolling back only the floor file also rejects,
//! * a corrupt floor file is never silently treated as 0,
//! * a missing floor file together with an existing blob is a contradiction
//!   and is rejected,
//! * first boot records a floor of 0; a counter history without a floor
//!   witness refuses to initialize.
//!
//! SIMULATION-MODE CAVEAT: this is best-effort, file-level rollback
//! detection. A coherent rollback of the WHOLE sealed-data directory (floor
//! + counter file together) resets both witnesses and cannot be caught by
//! any file-only mechanism; real rollback resistance requires SGX hardware
//! monotonic counters, which are not integrated in the strict hardware path
//! (documented gap — the file floor applies there as defense-in-depth).

use neo_tee::TeeEnclave;
use neo_tee::enclave::EnclaveConfig;
use neo_tee::enclave::rollback_floor::{
    FLOOR_FILE_NAME, floor_path, load as load_floor, store as store_floor,
};
use neo_tee::error::{EnclaveInitError, TeeError};
use neo_tee::wallet::SealedKey;

fn config(dir: &std::path::Path) -> EnclaveConfig {
    EnclaveConfig {
        sealed_data_path: dir.to_path_buf(),
        simulation: true,
        ..Default::default()
    }
}

/// Assert the error is the explicit fail-closed rollback error (not a
/// generic unseal/decrypt failure).
fn assert_rollback_rejected(result: Result<(), TeeError>) {
    match result {
        Ok(()) => panic!("unseal must fail closed after a rollback"),
        Err(TeeError::RollbackProtection(msg)) => {
            assert!(
                msg.contains("fail-closed"),
                "rollback error must state the fail-closed policy, got: {msg}"
            );
        }
        Err(other) => panic!("expected RollbackProtection, got: {other}"),
    }
}

/// T07-T4 (T43 fix): a sealed blob MUST be rejected after the host-side
/// monotonic counter is rolled back to zero.
///
/// The blob's replay floor is an external `.rollback_floor` witness that
/// survives the counter-file rollback, so `floor (>= 1) > enclave counter (0)`
/// is detected and the old blob fails to unseal. Previously the blob's own
/// counter was used as the floor, making the check a no-op.
#[test]
fn sealed_key_unseal_rejects_blob_after_counter_rollback() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path().to_path_buf();

    // Enclave A: seal a key, which bumps the monotonic counter to 1 and
    // atomically raises the external rollback floor to the same value.
    let enclave_a = TeeEnclave::new(config(&dir));
    enclave_a.initialize().expect("init A");
    let private_key = [0x42u8; 32];
    let public_key = [0x02u8; 33];
    let script_hash = [0xABu8; 20];
    let sealed = SealedKey::seal(
        &enclave_a,
        &private_key,
        &public_key,
        &script_hash,
        Some("t07".to_string()),
    )
    .expect("seal");

    let counter_at_seal = enclave_a.current_counter().expect("counter A");
    assert!(counter_at_seal >= 1, "sealing must bump the counter");
    assert_eq!(sealed.sealed_data.counter, counter_at_seal);

    // The external floor must have been persisted next to the blob, outside
    // of it, and must dominate the seal counter.
    let floor_file = floor_path(&dir);
    let floor_at_seal = load_floor(&floor_file)
        .expect("floor must be readable")
        .expect("seal must persist the rollback floor");
    assert_eq!(floor_at_seal, counter_at_seal);

    let key_path = dir.join("key.json");
    sealed.save_to_file(&key_path).expect("save");

    // Simulate a rollback / loss of the counter file only (attacker restores
    // an older copy of the sealed-data directory, or the file is truncated).
    // The floor witness is NOT rolled back with it.
    let counter_path = dir.join(".monotonic_counter");
    assert!(counter_path.exists(), "counter file should be persisted");
    std::fs::remove_file(&counter_path).expect("remove counter file");

    // Enclave B over the same (rolled-back) directory: counter restarts at 0.
    // A missing counter file is treated as "first boot" => Ok(()).
    let enclave_b = TeeEnclave::new(config(&dir));
    enclave_b.initialize().expect("init B");
    let counter_b = enclave_b.current_counter().expect("counter B");
    assert_eq!(counter_b, 0, "counter should have rolled back to 0");

    let reloaded = SealedKey::load_from_file(&key_path).expect("load");

    // SECURE (T43): reject. The external floor (1) is ahead of the enclave's
    // rolled-back counter (0) -> the directory was rolled back.
    assert_rollback_rejected(reloaded.unseal(&enclave_b).map(|_| ()));
}

/// T07-T4b (T43 fix): the replay floor is EXTERNAL, not self-referential.
///
/// Proof by direct manipulation of the floor witness: with the counter file
/// intact, lowering only the floor below the blob's counter must reject the
/// blob (the blob is no longer covered by the floor history), and restoring
/// the floor must make the very same blob acceptable again. The blob's own
/// counter never influences the decision.
#[test]
fn unseal_replay_floor_is_external_not_self_referential() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path().to_path_buf();
    let floor_file = floor_path(&dir);

    let enclave = TeeEnclave::new(config(&dir));
    enclave.initialize().expect("init");

    let private_key = [7u8; 32];
    let sealed =
        SealedKey::seal(&enclave, &private_key, &[0x03u8; 33], &[0x11u8; 20], None).expect("seal");
    assert!(sealed.sealed_data.counter >= 1);

    // Baseline: the freshly sealed key unseals fine.
    let unsealed = sealed.unseal(&enclave).expect("fresh seal must unseal");
    assert_eq!(&*unsealed, &private_key);

    // Roll back ONLY the floor file (counter file stays at the seal counter):
    // the blob's counter is now ahead of the floor -> reject.
    store_floor(&floor_file, 0).expect("write lowered floor");
    assert_rollback_rejected(sealed.unseal(&enclave).map(|_| ()));

    // Restore the floor: the identical blob is acceptable again. Acceptance
    // is driven by the external witness, not by the blob's own counter.
    store_floor(&floor_file, sealed.sealed_data.counter).expect("restore floor");
    let unsealed = sealed.unseal(&enclave).expect("unseal after floor restore");
    assert_eq!(&*unsealed, &private_key);
}

/// T43: a corrupt or unparsable floor witness must never be silently treated
/// as 0 — unseal fails closed with the explicit rollback error.
#[test]
fn unseal_fails_closed_when_floor_file_is_corrupt() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path().to_path_buf();

    let enclave = TeeEnclave::new(config(&dir));
    enclave.initialize().expect("init");
    let sealed =
        SealedKey::seal(&enclave, &[1u8; 32], &[0x02u8; 33], &[0x22u8; 20], None).expect("seal");

    // Corrupt the witness (truncated garbage — neither the magic/version nor
    // the digest can verify).
    std::fs::write(floor_path(&dir), b"not-a-floor-file").expect("corrupt floor");

    let err = sealed
        .unseal(&enclave)
        .expect_err("corrupt floor must fail closed");
    match err {
        TeeError::RollbackProtection(msg) => {
            assert!(
                msg.contains("corrupt"),
                "expected corrupt-floor error, got: {msg}"
            );
        }
        other => panic!("expected RollbackProtection, got: {other}"),
    }
}

/// T43: a missing floor together with an existing sealed blob is a
/// contradiction (every seal persists the floor). Conservative direction:
/// reject instead of treating the floor as 0.
#[test]
fn unseal_rejects_missing_floor_contradiction() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path().to_path_buf();

    let enclave = TeeEnclave::new(config(&dir));
    enclave.initialize().expect("init");
    let sealed =
        SealedKey::seal(&enclave, &[2u8; 32], &[0x02u8; 33], &[0x33u8; 20], None).expect("seal");

    std::fs::remove_file(floor_path(&dir)).expect("remove floor witness");

    let err = sealed
        .unseal(&enclave)
        .expect_err("missing floor with existing blob must fail closed");
    match err {
        TeeError::RollbackProtection(msg) => {
            assert!(
                msg.contains("missing"),
                "expected missing-floor error, got: {msg}"
            );
        }
        other => panic!("expected RollbackProtection, got: {other}"),
    }
}

/// T43: first-boot initialization is allowed and records a floor of 0.
#[test]
fn initialize_records_floor_witness_on_first_boot() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path().to_path_buf();

    assert!(
        !dir.join(FLOOR_FILE_NAME).exists(),
        "no floor witness before first boot"
    );

    let enclave = TeeEnclave::new(config(&dir));
    enclave.initialize().expect("init");

    assert_eq!(
        load_floor(&floor_path(&dir)).expect("floor readable"),
        Some(0),
        "first boot must record a floor of 0"
    );
}

/// T43: a monotonic-counter history without a floor witness is a
/// contradiction (legacy pre-T43 directory, or the floor was deleted to
/// weaken rollback detection) — initialization fails closed.
#[test]
fn initialize_rejects_counter_history_without_floor_witness() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path().to_path_buf();

    // Pre-existing counter history (as left behind by any pre-T43 run that
    // incremented the counter) but no `.rollback_floor` witness.
    let counter_path = dir.join(".monotonic_counter");
    std::fs::write(&counter_path, 5u64.to_le_bytes()).expect("write counter file");
    assert!(!dir.join(FLOOR_FILE_NAME).exists());

    let enclave = TeeEnclave::new(config(&dir));
    let err = enclave
        .initialize()
        .expect_err("counter history without floor must fail closed");
    match err {
        TeeError::EnclaveInitError { error, .. } => {
            assert_eq!(error, EnclaveInitError::RollbackFloorInvalid);
        }
        other => panic!("expected RollbackFloorInvalid, got: {other}"),
    }
    assert!(!enclave.is_ready());
}

/// T43: a corrupt floor witness also fails initialization (fail-closed), it
/// is not silently recreated as 0.
#[test]
fn initialize_fails_closed_on_corrupt_floor_witness() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path().to_path_buf();

    std::fs::write(floor_path(&dir), b"garbage").expect("write corrupt floor");

    let enclave = TeeEnclave::new(config(&dir));
    let err = enclave
        .initialize()
        .expect_err("corrupt floor witness must fail closed");
    match err {
        TeeError::EnclaveInitError { error, .. } => {
            assert_eq!(error, EnclaveInitError::RollbackFloorInvalid);
        }
        other => panic!("expected RollbackFloorInvalid, got: {other}"),
    }
    assert!(!enclave.is_ready());
}
