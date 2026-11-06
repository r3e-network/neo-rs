// Copyright (c) 2024 R3E Network
// Licensed under the MIT License. See LICENSE file for details.

//! Contract parameter type system for the protocol layer.
//!
//! - [`contract_parameter`]: the [`ContractParameter`] value container and its
//!   [`ContractParameterValue`] variants (matches C#
//!   `Neo.SmartContract.ContractParameter`).
//! - [`ContractParameterType`]: the parameter type enum, canonically owned by
//!   `neo-primitives` and re-exported here so the parameter type system presents
//!   a single, complete surface.

pub mod contract_parameter;

pub use contract_parameter::{ContractParameter, ContractParameterValue};
pub use neo_primitives::ContractParameterType;
