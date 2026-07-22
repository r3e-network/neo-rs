// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Minimal extension trait for [`neo_primitives::Verifiable`] containers.
//!
//! This trait exposes only state-independent payload facts. Execution-backed
//! witness verification belongs to `neo-execution`; node services such as
//! state-root verification resolve state-dependent verifier hashes before
//! constructing a verifiable wrapper.

use crate::VerifiableContainer;

/// Extension of [`neo_primitives::Verifiable`] for the payload layer.
///
/// Payload types implement this without depending on storage or the
/// application engine.
pub trait VerifiableExt: neo_primitives::Verifiable {
    /// Returns the script hashes that should be verified for this container.
    fn script_hashes_for_verifying(&self) -> Vec<neo_primitives::UInt160> {
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
}
