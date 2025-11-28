//! TEE Enclave abstraction
//!
//! Provides a unified interface for TEE operations, supporting both
//! simulation mode and real SGX hardware.

mod runtime;
mod sealing;

pub use runtime::{EnclaveConfig, EnclaveState, TeeEnclave};
pub use sealing::{seal_data, unseal_data, SealedData};
