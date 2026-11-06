// Copyright (c) 2024 R3E Network
// Licensed under the MIT License. See LICENSE file for details.

//! Fluent builder for protocol [`Witness`] payloads.

use super::ScriptBuilder;
use crate::payloads::Witness;
use neo_io::{IoError, IoResult};

/// Builder for `Witness` instances.
#[derive(Default)]
#[must_use]
pub struct WitnessBuilder {
    invocation: Vec<u8>,
    verification: Vec<u8>,
}

impl WitnessBuilder {
    /// Creates a new empty witness builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the invocation script (consuming builder).
    pub fn invocation_script(mut self, script: Vec<u8>) -> Self {
        self.invocation = script;
        self
    }

    /// Sets the verification script (consuming builder).
    pub fn verification_script(mut self, script: Vec<u8>) -> Self {
        self.verification = script;
        self
    }

    /// Adds an invocation script (mutable reference).
    pub fn add_invocation(&mut self, script: Vec<u8>) -> IoResult<&mut Self> {
        set_script_once(
            &mut self.invocation,
            script,
            "Invocation script already exists in the witness builder",
        )?;
        Ok(self)
    }

    /// Adds a verification script (mutable reference).
    pub fn add_verification(&mut self, script: Vec<u8>) -> IoResult<&mut Self> {
        set_script_once(
            &mut self.verification,
            script,
            "Verification script already exists in the witness builder",
        )?;
        Ok(self)
    }

    /// Builds an invocation script with a [`ScriptBuilder`] callback and adds it (mutable reference).
    pub fn add_invocation_with_builder<F>(&mut self, config: F) -> IoResult<&mut Self>
    where
        F: FnOnce(&mut ScriptBuilder),
    {
        let script = build_script(config);
        self.add_invocation(script)
    }

    /// Builds a verification script with a [`ScriptBuilder`] callback and adds it (mutable reference).
    pub fn add_verification_with_builder<F>(&mut self, config: F) -> IoResult<&mut Self>
    where
        F: FnOnce(&mut ScriptBuilder),
    {
        let script = build_script(config);
        self.add_verification(script)
    }

    /// Builds the witness, returning an empty witness when no scripts were set.
    pub fn build(&self) -> Witness {
        if self.invocation.is_empty() && self.verification.is_empty() {
            Witness::new()
        } else {
            Witness::new_with_scripts(self.invocation.clone(), self.verification.clone())
        }
    }
}

fn build_script(config: impl FnOnce(&mut ScriptBuilder)) -> Vec<u8> {
    let mut builder = ScriptBuilder::new();
    config(&mut builder);
    builder.to_array()
}

fn set_script_once(
    slot: &mut Vec<u8>,
    script: Vec<u8>,
    duplicate_message: &'static str,
) -> IoResult<()> {
    if !slot.is_empty() {
        return Err(IoError::invalid_data(duplicate_message));
    }
    *slot = script;
    Ok(())
}
