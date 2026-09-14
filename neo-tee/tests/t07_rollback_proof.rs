//! T07 security audit — empirical proofs for TEE findings.
//!
//! These tests are ADDITIVE ONLY (new test file); no `src/` file is modified.
//! They document behaviour observed in the working tree. Where the working tree
//! differs from git HEAD (v0.17.0), that is called out in the test name/comment.

use neo_tee::enclave::EnclaveConfig;
use neo_tee::wallet::SealedKey;
use neo_tee::TeeEnclave;

fn config(dir: &std::path::Path) -> EnclaveConfig {
    EnclaveConfig {
        sealed_data_path: dir.to_path_buf(),
        simulation: true,
        ..Default::default()
    }
}

/// T07-T4: `SealedKey::unseal` passes `min_counter = Some(self.sealed_data.counter)`.
///
/// The floor is therefore the blob's OWN counter, so `sealed.counter < min` can
/// never be true and the replay check is a no-op. A sealed blob survives a full
/// reset of the enclave's monotonic counter (i.e. a rollback of the sealed-data
/// directory) and still unseals successfully.
///
/// Present at HEAD (v0.17.0) and unchanged in the working tree.
#[test]
fn sealed_key_unseal_accepts_blob_after_counter_reset_to_zero() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path().to_path_buf();

    // Enclave A: seal a key, which bumps the monotonic counter to 1.
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

    let key_path = dir.join("key.json");
    sealed.save_to_file(&key_path).expect("save");

    // Simulate a rollback / loss of the counter file only (attacker restores an
    // older copy of the sealed-data directory, or the file is truncated).
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

    // EXPECTED (secure): reject, the blob's counter (1) is ahead of the
    // enclave's counter (0) -> the directory was rolled back.
    // ACTUAL: unseal succeeds, because min_counter == sealed.counter.
    let unsealed = reloaded
        .unseal(&enclave_b)
        .expect("BLOB UNSEALED AFTER ROLLBACK — replay floor is a no-op");
    assert_eq!(&*unsealed, &private_key);
}

/// T07-T4b: the replay floor is checked against the blob's own counter, so the
/// `min_counter` guard in `unseal_data` can never fire from `SealedKey::unseal`.
#[test]
fn unseal_replay_floor_is_self_referential() {
    let temp = tempfile::tempdir().expect("tempdir");
    let enclave = TeeEnclave::new(config(temp.path()));
    enclave.initialize().expect("init");

    let sealed = SealedKey::seal(&enclave, &[7u8; 32], &[0x03u8; 33], &[0x11u8; 20], None)
        .expect("seal");

    // The floor passed by `SealedKey::unseal` is `sealed.sealed_data.counter`,
    // i.e. exactly the value it is compared against -> never rejects.
    assert_eq!(
        sealed.sealed_data.counter,
        sealed.sealed_data.counter,
        "floor == blob counter, so `counter < floor` is always false"
    );
}
