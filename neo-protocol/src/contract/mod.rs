// Copyright (c) 2024 R3E Network
// Licensed under the MIT License. See LICENSE file for details.

//! Contract construction helpers for the protocol layer.
//!
//! - [`helpers`]: the [`Contract`] type and its redeem-script builders
//!   (matches C# `Neo.SmartContract.Contract`), plus the self-contained
//!   [`ContractError`].
//! - [`witness_scripts`]: pure witness-script builders and classifiers
//!   (the VM-level portion of C# `Neo.SmartContract.Helper`).

pub mod helpers;
pub mod witness_scripts;

pub use helpers::{Contract, ContractError};
pub use witness_scripts::WitnessScript;
