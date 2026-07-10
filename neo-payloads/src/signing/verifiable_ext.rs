// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Minimal extension trait for [`neo_primitives::Verifiable`] containers.
//!
//! The full smart-contract-engine-based verification helpers
//! (`script_hashes_for_verifying`, `verify_witnesses`) are defined in
//! `neo-core` because they need `ApplicationEngine` and the native
//! contracts. The payload layer only depends on the basic
//! [`neo_primitives::Verifiable`] trait from `neo-primitives`.

use neo_storage::{CacheRead, DataCache};

use crate::VerifiableContainer;

/// Extension of [`neo_primitives::Verifiable`] for the payload layer.
///
/// The full implementation is in `neo-core`; this trait exists so that
/// payload types can be marked as "extensible verifiable" without
/// pulling in the smart-contract engine.
pub trait VerifiableExt: neo_primitives::Verifiable {
    /// Returns the script hashes that should be verified for this container.
    fn script_hashes_for_verifying<B: CacheRead>(
        &self,
        _snapshot: &DataCache<B>,
    ) -> Vec<neo_primitives::UInt160> {
        Vec::new()
    }

    /// Returns the witnesses associated with this container.
    fn witnesses(&self) -> Vec<&crate::Witness> {
        Vec::new()
    }

    /// Returns mutable access to the witnesses associated with this container.
    fn witnesses_mut(&mut self) -> Vec<&mut crate::Witness> {
        Vec::new()
    }

    /// Returns this payload as a transaction when applicable.
    fn as_transaction(&self) -> Option<&crate::Transaction> {
        None
    }

    /// Returns the actual payload object to install as
    /// `ApplicationEngine.ScriptContainer` during witness verification.
    ///
    /// C# `Helper.VerifyWitness` passes the `IVerifiable` itself into the
    /// verification engine. Payloads that can cheaply clone themselves should
    /// override this so verification scripts observe the same script container
    /// through `GetScriptContainer`, `CurrentSigners`, and `CheckWitness`.
    fn to_verifiable_container(&self) -> Option<std::sync::Arc<VerifiableContainer>> {
        None
    }

    /// Verifies witnesses against the provided protocol settings and snapshot.
    fn verify_witnesses<B: CacheRead>(
        &self,
        _settings: &neo_config::ProtocolSettings,
        _snapshot: &DataCache<B>,
        _max_gas: i64,
    ) -> bool {
        true
    }
}
