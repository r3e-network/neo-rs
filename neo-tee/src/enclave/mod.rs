//! TEE Enclave abstraction
//!
//! Provides a unified interface for TEE operations, supporting both
//! simulation mode and real SGX hardware.

mod runtime;
mod sealing;

pub use runtime::{EnclaveConfig, EnclaveState, InitResult, TeeEnclave};
pub use sealing::{
    KeyDerivationParams, SealedData, SecureKey, derive_key_hkdf, derive_sealing_key, seal_data,
    seal_data_with_context, unseal_data,
};
