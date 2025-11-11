#![cfg_attr(not(feature = "std"), no_std)]

//! Contract utilities for the Neo N3 Rust implementation.
//!
//! The crate focuses on contract metadata (`ContractManifest`), native contract
//! registration, and a minimal runtime that interacts with the storage layer.

extern crate alloc;

pub mod engine;
pub mod error;
pub mod manifest;
pub mod native;
pub mod runtime;
pub mod script_decoder;

pub use engine::{ApplicationEngine, EngineConfig};
pub use manifest::{ContractManifest, ContractMethod, ContractParameter};
pub use native::{NativeContract, NativeRegistry};
pub use runtime::{ExecutionContext, GasMeter, InvocationResult};
