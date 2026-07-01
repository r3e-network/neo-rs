//! # neo-hsm::settings
//!
//! HSM provider settings and signing profile records.
//!
//! ## Boundary
//!
//! This module belongs to `neo-hsm`. This adapter crate owns signing-provider
//! integration and must not own consensus, ledger persistence, or node
//! orchestration.
//!
//! ## Contents
//!
//! - `config`: HSM provider configuration and signing profile records.

pub mod config;

pub use config::{HsmConfig, HsmProvider, ProviderProfile, SigFormat, profile};
