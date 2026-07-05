//! # neo-execution::contracts
//!
//! Contract metadata, manifests, deployed-state records, and contract parameter
//! types.
//!
//! ## Boundary
//!
//! This module belongs to `neo-execution`. This execution crate owns VM/native
//! interop behavior and must not own durable storage engines, P2P sync, or
//! application startup.
//!
//! ## Contents
//!
//! - `contract`: contract records and script-hash helpers.
//! - `contract_parameter`: contract parameter value records.
//! - `contract_parameters_context`: contract parameter signing context records.
//! - `contract_state`: deployed contract state records.
//! - `deployed_contract`: deployed contract wrapper records.

pub mod contract;
pub mod contract_parameter;
pub mod contract_parameters_context;
pub mod contract_state;
pub mod deployed_contract;
