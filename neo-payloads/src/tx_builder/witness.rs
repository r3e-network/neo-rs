use crate::Witness;
use neo_error::CoreError;
use neo_vm::script_builder::ScriptBuilder;

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

    /// Builds and adds an invocation script using a [`ScriptBuilder`].
    pub fn add_invocation_with_builder<F>(&mut self, config: F) -> Result<&mut Self, CoreError>
    where
        F: FnOnce(&mut ScriptBuilder),
    {
        let script = build_script(config);
        self.add_invocation(script)
    }

    /// Builds and adds a verification script using a [`ScriptBuilder`].
    pub fn add_verification_with_builder<F>(&mut self, config: F) -> Result<&mut Self, CoreError>
    where
        F: FnOnce(&mut ScriptBuilder),
    {
        let script = build_script(config);
        self.add_verification(script)
    }

    /// Builds and returns the configured witness.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_builder_produces_empty_witness() {
        let w = WitnessBuilder::new().build();
        assert!(w.invocation_script().is_empty());
        assert!(w.verification_script().is_empty());
    }

    #[test]
    fn consuming_setters_populate_scripts() {
        let w = WitnessBuilder::new()
            .invocation_script(vec![1, 2, 3])
            .verification_script(vec![4, 5])
            .build();
        assert_eq!(w.invocation_script(), &[1, 2, 3]);
        assert_eq!(w.verification_script(), &[4, 5]);
    }

    #[test]
    fn add_is_set_once_per_slot() {
        let mut b = WitnessBuilder::new();
        b.add_invocation(vec![1]).unwrap();
        // A second add for an already-populated slot is rejected.
        assert!(b.add_invocation(vec![2]).is_err());
        // The verification slot is independent and still settable.
        b.add_verification(vec![9]).unwrap();
        let w = b.build();
        assert_eq!(w.invocation_script(), &[1]);
        assert_eq!(w.verification_script(), &[9]);
    }

    #[test]
    fn add_with_builder_emits_script_bytes() {
        let mut b = WitnessBuilder::new();
        b.add_verification_with_builder(|sb| {
            sb.emit_push_int(1);
        })
        .unwrap();
        // The closure-built verification script is non-empty.
        assert!(!b.build().verification_script().is_empty());
    }
}
