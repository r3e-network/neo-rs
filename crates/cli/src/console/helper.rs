use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use neo_core::neo_vm::{Script, VmError};
use neo_core::smart_contract::manifest::{ContractAbi, ContractEventDescriptor};
use std::collections::HashSet;

/// Extension trait that mirrors the `Helper.IsYes` C# helper.
pub trait StringPromptExt {
    /// Returns `true` when the input is `"yes"` or `"y"` (case-insensitive).
    fn is_yes(&self) -> bool;
}

impl StringPromptExt for str {
    fn is_yes(&self) -> bool {
        matches!(self.trim().to_ascii_lowercase().as_str(), "yes" | "y")
    }
}

impl StringPromptExt for String {
    fn is_yes(&self) -> bool {
        self.as_str().is_yes()
    }
}

/// Wraps `Convert.ToBase64String` semantics from the C# helper.
pub fn to_base64_string(input: &[u8]) -> String {
    BASE64_STANDARD.encode(input)
}

/// Validates a contract script and ABI pair, mirroring
/// `Neo.SmartContract.Helper.Check`.
pub struct ContractScriptValidator;

impl ContractScriptValidator {
    /// Ensures that each ABI method offset points to a valid instruction and
    /// that the ABI metadata contains unique method/event definitions.
    pub fn validate(script: &[u8], abi: &ContractAbi) -> Result<()> {
        let strict_script = Script::new(script.to_vec(), true).map_err(|err| {
            anyhow!(
                "Contract script validation failed. \
                 The provided script or manifest format is invalid and cannot be processed. \
                 Please verify the script bytecode and manifest are correctly formatted and compatible. \
                 Original error: {err}"
            )
        })?;

        for method in &abi.methods {
            let offset = usize::try_from(method.offset).context(format!(
                "Contract method '{}' specifies a negative offset ({})",
                method.name, method.offset
            ))?;
            strict_script
                .get_instruction(offset)
                .map_err(|err| format_vm_error(err, &method.name, offset))?;
        }

        Self::ensure_unique_methods(abi)?;
        Self::ensure_unique_events(&abi.events)?;
        Ok(())
    }

    fn ensure_unique_methods(abi: &ContractAbi) -> Result<()> {
        let mut seen = HashSet::new();
        for method in &abi.methods {
            let key = (method.name.clone(), method.parameters.len() as i32);
            if !seen.insert(key) {
                return Err(anyhow!(
                    "Contract ABI contains duplicate method definitions for '{}'",
                    method.name
                ));
            }
        }
        Ok(())
    }

    fn ensure_unique_events(events: &[ContractEventDescriptor]) -> Result<()> {
        let mut seen = HashSet::new();
        for event in events {
            if !seen.insert(event.name.clone()) {
                return Err(anyhow!(
                    "Contract ABI contains duplicate event definitions for '{}'",
                    event.name
                ));
            }
        }
        Ok(())
    }
}

fn format_vm_error(err: VmError, method_name: &str, offset: usize) -> anyhow::Error {
    anyhow!(
        "Contract script validation failed for method '{}' at offset {}. \
         Original error: {err}",
        method_name,
        offset
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_core::smart_contract::manifest::{ContractEventDescriptor, ContractMethodDescriptor};
    use neo_core::smart_contract::ContractParameterType;

    #[test]
    fn is_yes_matches_csharp_helper() {
        assert!("yes".is_yes());
        assert!("Y".to_string().is_yes());
        assert!(!"no".is_yes());
        assert!(!"".is_yes());
    }

    #[test]
    fn base64_wrapper_matches_std() {
        let data = [0u8, 1, 2, 3];
        assert_eq!(to_base64_string(&data), "AAECAw==");
    }

    #[test]
    fn validator_detects_duplicate_methods() {
        let mut abi = ContractAbi::default();
        abi.methods = vec![
            ContractMethodDescriptor::new(
                "foo".to_string(),
                Vec::new(),
                ContractParameterType::Void,
                0,
                false,
            )
            .unwrap(),
            ContractMethodDescriptor::new(
                "foo".to_string(),
                Vec::new(),
                ContractParameterType::Void,
                0,
                false,
            )
            .unwrap(),
        ];

        let script = vec![0x51]; // PUSH1
        let error = ContractScriptValidator::validate(&script, &abi).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Contract ABI contains duplicate method"),
            "{error}"
        );
    }

    #[test]
    fn validator_detects_duplicate_events() {
        let mut abi = ContractAbi::default();
        abi.events = vec![
            ContractEventDescriptor::new("evt".to_string(), Vec::new()).unwrap(),
            ContractEventDescriptor::new("evt".to_string(), Vec::new()).unwrap(),
        ];
        // Script still needs to be syntactically valid.
        let script = vec![0x51];
        let error = ContractScriptValidator::validate(&script, &abi).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Contract ABI contains duplicate event"),
            "{error}"
        );
    }
}
