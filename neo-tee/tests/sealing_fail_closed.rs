//! C-2 regression: sealing-key derivation fails closed unless a sealing
//! backend was compiled in.
//!
//! This is an integration test on purpose. Unit tests inside the crate are
//! compiled with `cfg(test)`, which enables the simulated key, so they cannot
//! observe what a dependent crate gets. An integration test links the library
//! the way a downstream crate does: without `cfg(test)`.
//!
//! `tee-sgx` builds are covered by the unit tests in `enclave/runtime.rs`.
#![cfg(not(feature = "tee-sgx"))]

use neo_tee::enclave::EnclaveState;
use neo_tee::{EnclaveConfig, TeeEnclave};

fn enclave(sealed_data_path: &std::path::Path, simulation: bool) -> TeeEnclave {
    TeeEnclave::new(EnclaveConfig {
        sealed_data_path: sealed_data_path.to_path_buf(),
        simulation,
        ..Default::default()
    })
}

/// The build every production binary gets: no `tee-sgx`, no `tee-sgx-sim`.
/// Asking for simulation in the runtime configuration must not be enough to
/// obtain a software sealing key.
#[cfg(not(feature = "tee-sgx-sim"))]
#[test]
fn a_build_without_a_sealing_backend_refuses_to_initialize() {
    use neo_tee::{EnclaveInitError, TeeError};

    for simulation in [true, false] {
        let dir = tempfile::tempdir().expect("tempdir");
        let enclave = enclave(dir.path(), simulation);

        match enclave.initialize() {
            Err(TeeError::EnclaveInitError { error, context }) => {
                assert_eq!(error, EnclaveInitError::SealingKeyDerivationFailed);
                assert!(
                    context.contains("tee-sgx"),
                    "the refusal names the missing feature: {context}"
                );
            }
            other => panic!("simulation={simulation}: expected a sealing refusal, got {other:?}"),
        }

        assert_eq!(enclave.state(), EnclaveState::Error);
        assert!(!enclave.is_ready());
        assert!(enclave.last_init_error().is_some());
        assert!(enclave.increment_counter().is_err());
        // No key material was derived, so no machine identifier was minted.
        assert!(!dir.path().join(".machine_id").exists());
    }
}

/// The opt-in used by downstream test suites.
#[cfg(feature = "tee-sgx-sim")]
#[test]
fn the_simulation_feature_enables_the_software_sealing_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    let enclave = enclave(dir.path(), true);

    let result = enclave.initialize().expect("simulated enclave initializes");

    assert!(result.sealing_key_derived);
    assert!(!result.hardware_attestation_available);
    assert_eq!(enclave.state(), EnclaveState::Ready);
}
