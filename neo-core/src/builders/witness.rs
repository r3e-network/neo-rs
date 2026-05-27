use crate::error::CoreError;
use crate::network::p2p::payloads::Witness;
use crate::script_builder::ScriptBuilder;

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
    pub fn add_invocation(&mut self, script: Vec<u8>) -> Result<&mut Self, CoreError> {
        set_script_once(
            &mut self.invocation,
            script,
            "Invocation script already exists in the witness builder",
        )?;
        Ok(self)
    }

    /// Adds a verification script (mutable reference).
    pub fn add_verification(&mut self, script: Vec<u8>) -> Result<&mut Self, CoreError> {
        set_script_once(
            &mut self.verification,
            script,
            "Verification script already exists in the witness builder",
        )?;
        Ok(self)
    }

    pub fn add_invocation_with_builder<F>(&mut self, config: F) -> Result<&mut Self, CoreError>
    where
        F: FnOnce(&mut ScriptBuilder),
    {
        let script = build_script(config);
        self.add_invocation(script)
    }

    pub fn add_verification_with_builder<F>(&mut self, config: F) -> Result<&mut Self, CoreError>
    where
        F: FnOnce(&mut ScriptBuilder),
    {
        let script = build_script(config);
        self.add_verification(script)
    }

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
) -> Result<(), CoreError> {
    if !slot.is_empty() {
        return Err(CoreError::invalid_operation(duplicate_message));
    }
    *slot = script;
    Ok(())
}
