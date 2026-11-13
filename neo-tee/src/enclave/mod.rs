//! TEE Enclave abstraction
//!
//! Provides a unified interface for TEE operations, supporting both
//! simulation mode and real SGX hardware.

pub mod rollback_floor;
mod runtime;
mod sealing;

pub use rollback_floor::{FLOOR_FILE_NAME, floor_path};
pub use runtime::{EnclaveConfig, EnclaveState, InitResult, TeeEnclave};
pub use sealing::{
    KeyDerivationParams, SealedData, SecureKey, derive_key_hkdf, derive_sealing_key, seal_data,
    seal_data_with_context, unseal_data,
};
