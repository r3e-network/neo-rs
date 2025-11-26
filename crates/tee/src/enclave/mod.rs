//! TEE Enclave abstraction
//!
//! Provides a unified interface for TEE operations, supporting both
//! simulation mode and real SGX hardware.

mod runtime;
mod sealing;

pub use runtime::{TeeEnclave, EnclaveConfig, EnclaveState};
pub use sealing::{SealedData, seal_data, unseal_data};
